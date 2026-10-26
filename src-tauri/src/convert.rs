use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::process::{Command, CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

use crate::data;

// Argumentos de un motor, mezclando textos y rutas
macro_rules! args {
    ($($arg:expr),* $(,)?) => {
        vec![$(OsString::from($arg)),*]
    };
}

const IMAGES: &[&str] = &["jpg", "png", "gif", "webp", "bmp", "tiff", "heic", "avif"];
const IMAGE_OUTPUTS: &[&str] = &[
    "jpg", "png", "gif", "webp", "bmp", "tiff", "avif", "ico", "pdf",
];
const AUDIO: &[&str] = &[
    "mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "wma", "aiff",
];
const VIDEO: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "mpeg", "3gp",
];
const PANDOC_IN: &[&str] = &["docx", "odt", "rtf", "html", "md", "epub"];
const PANDOC_OUT: &[&str] = &["docx", "odt", "rtf", "txt", "html", "md", "epub"];
const OFFICE_DOCS_IN: &[&str] = &["docx", "doc", "odt", "rtf", "txt", "html"];
const OFFICE_DOCS_OUT: &[&str] = &["docx", "doc", "odt", "rtf", "txt", "html", "pdf", "epub"];
const SHEETS_IN: &[&str] = &["xlsx", "xls", "ods", "csv", "tsv"];
const SHEETS_OUT: &[&str] = &["xlsx", "xls", "ods", "csv", "pdf"];
const DATA: &[&str] = &["json", "xml", "yaml", "csv", "tsv"];
const SLIDES: &[&str] = &["pptx", "ppt", "odp"];
const ARCHIVES_IN: &[&str] = &["zip", "rar", "7z", "tar", "tar.gz"];
const ARCHIVES_OUT: &[&str] = &["zip", "7z", "tar", "tar.gz"];
const EBOOKS: &[&str] = &["epub", "mobi", "azw3", "fb2"];

const GIF_FILTER: &str =
    "fps=10,scale='min(480,iw)':-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse";
// x264 y VP9 exigen ancho y alto pares
const EVEN_SIZE: &str = "scale=trunc(iw/2)*2:trunc(ih/2)*2";

#[derive(Default)]
pub struct Jobs {
    running: Mutex<HashMap<u32, CommandChild>>,
    cancelled: Mutex<HashSet<u32>>,
}

#[derive(Debug)]
pub enum Failure {
    Cancelled,
    Unsupported,
    Missing,
    Unreadable,
    Protected,
    Links,
    NoSpace,
    Denied,
    Failed,
}

impl Failure {
    pub fn code(&self) -> &'static str {
        match self {
            Failure::Cancelled => "cancelled",
            Failure::Unsupported => "unsupported",
            Failure::Missing => "missing",
            Failure::Unreadable => "unreadable",
            Failure::Protected => "protected",
            Failure::Links => "failed",
            Failure::NoSpace => "nospace",
            Failure::Denied => "denied",
            Failure::Failed => "failed",
        }
    }
}

#[derive(Clone, Serialize)]
struct Progress {
    job: u32,
    fraction: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Engine {
    Ffmpeg,
    Vips,
    HeicThenVips,
    Pandoc,
    Office,
    PandocThenOffice,
    OfficeThenPandoc,
    OfficeThenData,
    Data,
    PdfText,
    PdfPages,
    SlidesToImages,
    Calibre,
    Archive,
}

fn pick_engine(from: &str, to: &str) -> Option<Engine> {
    let is = |list: &[&str], ext: &str| list.contains(&ext);
    let engine = match (from, to) {
        _ if is(AUDIO, from) && is(AUDIO, to) => Engine::Ffmpeg,
        _ if is(VIDEO, from) && (is(VIDEO, to) || is(AUDIO, to)) => Engine::Ffmpeg,
        (_, "gif" | "png" | "jpg") if is(VIDEO, from) => Engine::Ffmpeg,
        // El vips empaquetado no trae decodificador HEVC; FFmpeg sí
        ("heic", _) if is(IMAGE_OUTPUTS, to) => Engine::HeicThenVips,
        _ if is(IMAGES, from) && is(IMAGE_OUTPUTS, to) => Engine::Vips,
        ("svg", "png" | "jpg" | "webp" | "pdf") => Engine::Vips,
        _ if is(EBOOKS, from) && (is(EBOOKS, to) || to == "pdf") => Engine::Calibre,
        ("pdf", "txt") => Engine::PdfText,
        ("pdf", "png" | "jpg") => Engine::PdfPages,
        ("pdf", "docx") => Engine::Office,
        _ if is(SLIDES, from) && (is(SLIDES, to) || to == "pdf") => Engine::Office,
        (_, "png" | "jpg") if is(SLIDES, from) => Engine::SlidesToImages,
        _ if is(DATA, from) && is(DATA, to) => Engine::Data,
        ("xlsx" | "xls" | "ods", "json" | "tsv") => Engine::OfficeThenData,
        _ if is(SHEETS_IN, from) && is(SHEETS_OUT, to) => Engine::Office,
        _ if is(ARCHIVES_IN, from) && is(ARCHIVES_OUT, to) => Engine::Archive,
        _ if is(PANDOC_IN, from) && is(PANDOC_OUT, to) => Engine::Pandoc,
        _ if is(OFFICE_DOCS_IN, from) && is(OFFICE_DOCS_OUT, to) => Engine::Office,
        // Pandoc no conoce DOC ni crea PDF, y LibreOffice no lee Markdown: se encadenan
        ("md", "doc" | "pdf") => Engine::PandocThenOffice,
        ("doc" | "txt", "md") => Engine::OfficeThenPandoc,
        _ => return None,
    };
    Some(engine)
}

// Qué programa falta en esta instalación para hacer la conversión, si falta alguno
pub fn missing_tool(app: &AppHandle, from: &str, to: &str) -> Option<&'static str> {
    match pick_engine(from, to)? {
        Engine::Office
        | Engine::PandocThenOffice
        | Engine::OfficeThenPandoc
        | Engine::OfficeThenData
        | Engine::SlidesToImages
            if office_program(app).is_none() =>
        {
            Some("libreoffice")
        }
        Engine::Calibre if calibre_program(app).is_none() => Some("calibre"),
        _ => None,
    }
}

pub fn normalize_ext(path: &Path) -> String {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return "tar.gz".into();
    }
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    match ext.as_str() {
        "jpeg" | "jpe" => "jpg",
        "tif" => "tiff",
        "heif" => "heic",
        "aif" => "aiff",
        "mpg" => "mpeg",
        "htm" => "html",
        "yml" => "yaml",
        "markdown" => "md",
        other => other,
    }
    .to_string()
}

fn base_name(input: &Path) -> String {
    let name = input.file_name().unwrap_or_default().to_string_lossy();
    for suffix in [".tar.gz", ".tgz"] {
        let cut = name.len().saturating_sub(suffix.len());
        if name
            .get(cut..)
            .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
        {
            return name[..cut].to_string();
        }
    }
    input
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

// Nunca sobrescribe: "foto.png", "foto (1).png", "foto (2).png"...
fn free_path(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let with = |name: &str| match ext {
        "" => dir.join(name),
        _ => dir.join(format!("{name}.{ext}")),
    };
    let mut path = with(base);
    let mut n = 1;
    while path.exists() {
        path = with(&format!("{base} ({n})"));
        n += 1;
    }
    path
}

pub async fn convert(
    app: &AppHandle,
    job: u32,
    input: &Path,
    to: &str,
    out_dir: &Path,
) -> Result<PathBuf, Failure> {
    let from = normalize_ext(input);
    let engine = pick_engine(&from, to).ok_or(Failure::Unsupported)?;
    let base = base_name(input);
    // Vídeos, PDF y presentaciones a imagen dan varias imágenes: van a una carpeta propia
    let folder = match engine {
        Engine::Ffmpeg if matches!(to, "png" | "jpg") => Some("frames"),
        Engine::PdfPages => Some("pages"),
        Engine::SlidesToImages => Some("slides"),
        _ => None,
    };
    let output = match folder {
        Some(kind) => free_path(out_dir, &format!("{base}_{kind}"), ""),
        None => free_path(out_dir, &base, to),
    };
    // Los pasos intermedios se hacen en el mismo disco que el destino, así mover es instantáneo
    let work = out_dir.join(format!(".swayfi-{}-{job}", std::process::id()));
    fs::create_dir_all(&work).map_err(|e| io_failure(&e))?;
    if folder.is_some() {
        fs::create_dir(&output).map_err(|e| io_failure(&e))?;
    }

    let result = run_engine(app, job, engine, input, &from, to, &output, &work).await;

    let _ = fs::remove_dir_all(&work);
    app.state::<Jobs>().cancelled.lock().unwrap().remove(&job);
    if result.is_err() {
        // Lo que haya quedado a medias se borra; free_path garantiza que no existía antes
        let _ = if folder.is_some() {
            fs::remove_dir_all(&output)
        } else {
            fs::remove_file(&output)
        };
    }
    result.map(|()| output)
}

#[allow(clippy::too_many_arguments)]
async fn run_engine(
    app: &AppHandle,
    job: u32,
    engine: Engine,
    input: &Path,
    from: &str,
    to: &str,
    output: &Path,
    work: &Path,
) -> Result<(), Failure> {
    match engine {
        Engine::Ffmpeg => run_ffmpeg(app, job, input, to, output).await,
        Engine::Vips => run_vips(app, job, input, from, to, output, work).await,
        Engine::HeicThenVips => {
            let png = work.join("decoded.png");
            let decode = args![
                "-hide_banner",
                "-nostdin",
                "-v",
                "error",
                "-i",
                input,
                "-frames:v",
                "1",
                "-update",
                "1",
                &png,
            ];
            run(app, job, sidecar(app, "ffmpeg")?.args(decode), |_| {}).await?;
            run_vips(app, job, &png, "png", to, output, work).await
        }
        Engine::Pandoc => run_pandoc(app, job, input, from, to, output).await,
        Engine::Office => {
            let produced = run_office(app, job, input, from, to, work).await?;
            move_into_place(&produced, output)
        }
        Engine::PandocThenOffice => {
            let docx = work.join("document.docx");
            run_pandoc(app, job, input, from, "docx", &docx).await?;
            let produced = run_office(app, job, &docx, "docx", to, work).await?;
            move_into_place(&produced, output)
        }
        Engine::OfficeThenPandoc => {
            let docx = run_office(app, job, input, from, "docx", work).await?;
            run_pandoc(app, job, &docx, "docx", to, output).await
        }
        Engine::OfficeThenData => {
            let csv = run_office(app, job, input, from, "csv", work).await?;
            run_data(app, job, &csv, "csv", to, output).await
        }
        Engine::Data => run_data(app, job, input, from, to, output).await,
        Engine::PdfText => {
            let command = sidecar(app, "mutool")?.args(args!["draw", "-q", "-o", output, input]);
            run(app, job, command, |_| {}).await
        }
        Engine::PdfPages => run_pdf_pages(app, job, input, to, output).await,
        Engine::SlidesToImages => {
            let pdf = run_office(app, job, input, from, "pdf", work).await?;
            run_pdf_pages(app, job, &pdf, to, output).await
        }
        Engine::Calibre => run_calibre(app, job, input, output).await,
        Engine::Archive => run_archive(app, job, input, from, to, output, work).await,
    }
}

pub fn cancel(app: &AppHandle, job: u32) {
    let jobs = app.state::<Jobs>();
    jobs.cancelled.lock().unwrap().insert(job);
    let child = jobs.running.lock().unwrap().remove(&job);
    if let Some(child) = child {
        let _ = child.kill();
    }
}

// Los motores van junto al ejecutable; en Linux los instala el paquete .deb como dependencias
fn sidecar(app: &AppHandle, name: &str) -> Result<Command, Failure> {
    let exe = std::env::current_exe().map_err(|_| Failure::Missing)?;
    let bundled = exe
        .with_file_name(name)
        .with_extension(std::env::consts::EXE_EXTENSION);
    if bundled.is_file() {
        app.shell().sidecar(name).map_err(|_| Failure::Missing)
    } else {
        Ok(app.shell().command(name))
    }
}

fn emit_progress(app: &AppHandle, job: u32, fraction: f64) {
    let fraction = fraction.clamp(0.0, 1.0);
    let _ = app.emit("progress", Progress { job, fraction });
}

fn move_into_place(produced: &Path, output: &Path) -> Result<(), Failure> {
    fs::rename(produced, output).map_err(|e| io_failure(&e))
}

struct Probe {
    duration: Option<f64>,
    video: Option<String>,
    audio: Option<String>,
}

// "ffmpeg -i archivo" sin salida termina con error, pero antes describe el archivo
async fn probe(app: &AppHandle, input: &Path) -> Result<Probe, Failure> {
    let output = sidecar(app, "ffmpeg")?
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(input)
        .output()
        .await
        .map_err(|_| Failure::Failed)?;
    let text = String::from_utf8_lossy(&output.stderr);
    if !text.contains("Stream #") {
        return Err(Failure::Unreadable);
    }
    Ok(Probe {
        duration: parse_duration(&text),
        video: codec_after(&text, "Video: "),
        audio: codec_after(&text, "Audio: "),
    })
}

fn parse_duration(text: &str) -> Option<f64> {
    let stamp = text.split("Duration: ").nth(1)?.split(',').next()?;
    let mut seconds = 0.0;
    for part in stamp.trim().split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(seconds).filter(|s| *s > 0.0)
}

fn codec_after(text: &str, label: &str) -> Option<String> {
    let rest = text.split(label).nth(1)?;
    rest.split([' ', ',']).next().map(str::to_string)
}

// Si el códec ya vale para el contenedor de destino se copia tal cual: es mucho más rápido
fn can_copy(to: &str, codec: Option<&str>, is_video: bool) -> bool {
    let Some(codec) = codec else { return true };
    let allowed: &[&str] = match (to, is_video) {
        ("mkv", _) => return true,
        ("mp4" | "mov", true) => &["h264", "hevc", "av1", "mpeg4"],
        ("mp4" | "mov", false) => &["aac", "mp3", "ac3", "eac3"],
        ("webm", true) => &["vp8", "vp9", "av1"],
        ("webm", false) => &["opus", "vorbis"],
        _ => &[],
    };
    allowed.contains(&codec)
}

fn audio_codec(to: &str) -> &'static [&'static str] {
    match to {
        "mp3" => &["-c:a", "libmp3lame", "-q:a", "2"],
        "wav" => &["-c:a", "pcm_s16le"],
        "flac" => &["-c:a", "flac"],
        "ogg" => &["-c:a", "libvorbis", "-q:a", "5"],
        "opus" | "webm" => &["-c:a", "libopus", "-b:a", "128k"],
        "wma" | "wmv" => &["-c:a", "wmav2", "-b:a", "192k"],
        "aiff" => &["-c:a", "pcm_s16be"],
        "avi" => &["-c:a", "libmp3lame", "-q:a", "3"],
        "mpeg" => &["-c:a", "mp2", "-b:a", "192k"],
        _ => &["-c:a", "aac", "-b:a", "192k"],
    }
}

fn video_codec(to: &str) -> &'static [&'static str] {
    match to {
        "webm" => &[
            "-vf",
            EVEN_SIZE,
            "-c:v",
            "libvpx-vp9",
            "-crf",
            "33",
            "-b:v",
            "0",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
            "-row-mt",
            "1",
        ],
        "avi" => &["-c:v", "mpeg4", "-q:v", "4"],
        "wmv" => &["-c:v", "wmv2", "-b:v", "4M"],
        // MPEG-2 solo admite frecuencias estándar
        "mpeg" => &["-c:v", "mpeg2video", "-q:v", "4", "-r", "25"],
        _ => &[
            "-vf", EVEN_SIZE, "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-pix_fmt",
            "yuv420p",
        ],
    }
}

fn ffmpeg_output_args(to: &str, probe: &Probe) -> Vec<&'static str> {
    if AUDIO.contains(&to) {
        return [&["-vn"][..], audio_codec(to)].concat();
    }
    match to {
        "gif" => vec!["-an", "-vf", GIF_FILTER, "-loop", "0"],
        "png" => vec!["-an", "-vf", "fps=1"],
        "jpg" => vec!["-an", "-vf", "fps=1", "-q:v", "2"],
        _ => {
            let mut out = vec!["-sn", "-dn"];
            if can_copy(to, probe.video.as_deref(), true) {
                out.extend(["-c:v", "copy"]);
            } else {
                out.extend(video_codec(to));
            }
            if can_copy(to, probe.audio.as_deref(), false) {
                out.extend(["-c:a", "copy"]);
            } else {
                out.extend(audio_codec(to));
            }
            out
        }
    }
}

async fn run_ffmpeg(
    app: &AppHandle,
    job: u32,
    input: &Path,
    to: &str,
    output: &Path,
) -> Result<(), Failure> {
    let probe = probe(app, input).await?;
    let mut list = args![
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-n",
        "-progress",
        "pipe:1",
        "-nostats",
        "-i",
        input,
    ];
    list.extend(
        ffmpeg_output_args(to, &probe)
            .into_iter()
            .map(OsString::from),
    );
    if matches!(to, "png" | "jpg") {
        list.push(output.join(format!("frame_%05d.{to}")).into_os_string());
    } else {
        list.push(output.as_os_str().to_owned());
    }

    run(app, job, sidecar(app, "ffmpeg")?.args(list), |line| {
        let micros = line
            .strip_prefix("out_time_us=")
            .and_then(|v| v.parse::<f64>().ok());
        if let (Some(micros), Some(total)) = (micros, probe.duration) {
            emit_progress(app, job, micros / 1e6 / total);
        }
    })
    .await
}

fn vips(app: &AppHandle) -> Result<Command, Failure> {
    let command = sidecar(app, "vips")?;
    // En macOS los módulos de vips y los formatos de ImageMagick viajan en Resources
    #[cfg(target_os = "macos")]
    let command = {
        let res = app.path().resource_dir().map_err(|_| Failure::Missing)?;
        command
            .env("VIPSHOME", res.join("vips"))
            .env("MAGICK_CODER_MODULE_PATH", res.join("imagemagick/coders"))
            .env("MAGICK_CODER_FILTER_PATH", res.join("imagemagick/filters"))
            .env("MAGICK_CONFIGURE_PATH", res.join("imagemagick/config"))
    };
    Ok(command)
}

async fn run_vips(
    app: &AppHandle,
    job: u32,
    input: &Path,
    from: &str,
    to: &str,
    output: &Path,
    work: &Path,
) -> Result<(), Failure> {
    let mut source = input.as_os_str().to_owned();
    // n=-1 carga todos los fotogramas para que las animaciones sigan animadas
    if matches!(from, "gif" | "webp") && matches!(to, "gif" | "webp") {
        source.push("[n=-1]");
    }

    if matches!(to, "bmp" | "pdf" | "ico") {
        // Estos formatos solo los guarda ImageMagick (magicksave), que no aplana la transparencia
        let temp = work.join("image.v");
        let first = if to == "ico" {
            args!["thumbnail", &source, &temp, "256", "--size", "down"]
        } else {
            args!["flatten", &source, &temp, "--background", "255"]
        };
        run(app, job, vips(app)?.args(first), |_| {}).await?;
        let save = args!["magicksave", &temp, output, "--format", to];
        return run(app, job, vips(app)?.args(save), |_| {}).await;
    }

    let mut target = output.as_os_str().to_owned();
    if to == "jpg" {
        target.push("[background=255]");
    }
    let copy = args!["copy", &source, &target];
    run(app, job, vips(app)?.args(copy), |_| {}).await
}

async fn run_pandoc(
    app: &AppHandle,
    job: u32,
    input: &Path,
    from: &str,
    to: &str,
    output: &Path,
) -> Result<(), Failure> {
    let reader = match from {
        "md" => "markdown",
        other => other,
    };
    let writer = match to {
        "md" => "gfm",
        "txt" => "plain",
        other => other,
    };
    let mut list = args![
        input,
        "-f",
        reader,
        "-t",
        writer,
        "--standalone",
        "-o",
        output
    ];
    let dir = output.parent().unwrap_or(Path::new("."));
    // Las imágenes de un DOCX u ODT se guardan en una carpeta junto al Markdown o HTML
    if matches!(to, "md" | "html") && matches!(from, "docx" | "odt" | "epub") {
        let media = free_path(dir, &format!("{}_media", base_name(output)), "");
        let name = media.file_name().unwrap_or_default().to_string_lossy();
        list.push(format!("--extract-media={name}").into());
    }
    let command = sidecar(app, "pandoc")?.args(list).current_dir(dir);
    run(app, job, command, |_| {}).await
}

// Ruta del LibreOffice empaquetado; si no está, el del sistema
fn office_program(app: &AppHandle) -> Option<PathBuf> {
    find_program(
        app,
        // soffice.bin y no el lanzador: así, al cancelar, se mata el proceso que de verdad convierte
        &[
            "libreoffice/program/soffice.bin",
            "LibreOffice.app/Contents/MacOS/soffice",
        ],
        &[
            r"C:\Program Files\LibreOffice\program\soffice.bin",
            "/Applications/LibreOffice.app/Contents/MacOS/soffice",
            "/usr/lib/libreoffice/program/soffice.bin",
        ],
    )
}

fn calibre_program(app: &AppHandle) -> Option<PathBuf> {
    find_program(
        app,
        &[
            "calibre/ebook-convert.exe",
            "calibre/ebook-convert",
            "calibre.app/Contents/MacOS/ebook-convert",
        ],
        &[
            r"C:\Program Files\Calibre2\ebook-convert.exe",
            "/Applications/calibre.app/Contents/MacOS/ebook-convert",
            "/usr/bin/ebook-convert",
        ],
    )
}

fn find_program(app: &AppHandle, bundled: &[&str], system: &[&str]) -> Option<PathBuf> {
    let resources = app.path().resource_dir().ok();
    let bundled = bundled
        .iter()
        .filter_map(|path| resources.as_ref().map(|dir| dir.join(path)));
    let system = system.iter().map(PathBuf::from);
    bundled.chain(system).find(|path| path.is_file())
}

fn office_filter(from: &str, to: &str) -> String {
    match (from, to) {
        (_, "doc") => "doc:MS Word 97",
        (_, "docx") => "docx:MS Word 2007 XML",
        (_, "txt") => "txt:Text (encoded):UTF8",
        (_, "html") => "html:XHTML Writer File:UTF8",
        (_, "xls") => "xls:MS Excel 97",
        // Separador coma, comillas dobles y UTF-8
        (_, "csv") => "csv:Text - txt - csv (StarCalc):44,34,76,1",
        (_, "ppt") => "ppt:MS PowerPoint 97",
        (_, other) => other,
    }
    .to_string()
}

// LibreOffice decide el nombre de salida: se le deja una carpeta vacía y se recoge lo que haya
async fn run_office(
    app: &AppHandle,
    job: u32,
    input: &Path,
    from: &str,
    to: &str,
    work: &Path,
) -> Result<PathBuf, Failure> {
    let program = office_program(app).ok_or(Failure::Missing)?;
    let out = work.join(format!("office-{to}"));
    fs::create_dir_all(&out).map_err(|e| io_failure(&e))?;
    // Un perfil propio evita chocar con un LibreOffice que el usuario tenga abierto
    let profile = app
        .path()
        .app_local_data_dir()
        .map_err(|_| Failure::Failed)?
        .join("libreoffice-profile");
    let env = format!("-env:UserInstallation={}", file_url(&profile));
    // Al crear el perfil LibreOffice sale pidiendo reiniciarse y no convierte nada
    if !profile.exists() {
        let init = args!["--headless", "--terminate_after_init", &env];
        let _ = app.shell().command(&program).args(init).output().await;
    }

    let mut list = args!["--headless", "--norestore", "--nolockcheck", &env];
    let infilter = match from {
        "csv" => Some("CSV:44,34,76,1"),
        "tsv" => Some("CSV:9,34,76,1"),
        "pdf" => Some("writer_pdf_import"),
        _ => None,
    };
    if let Some(filter) = infilter {
        list.push(format!("--infilter={filter}").into());
    }
    list.extend(args![
        "--convert-to",
        office_filter(from, to),
        "--outdir",
        &out,
        input
    ]);
    run(app, job, app.shell().command(program).args(list), |_| {}).await?;

    // Si no puede abrir el archivo, LibreOffice termina bien pero sin escribir nada
    let produced = fs::read_dir(&out)
        .map_err(|e| io_failure(&e))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .next();
    produced.ok_or(Failure::Unreadable)
}

fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.replace('%', "%25").replace(' ', "%20");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

async fn run_pdf_pages(
    app: &AppHandle,
    job: u32,
    input: &Path,
    to: &str,
    output: &Path,
) -> Result<(), Failure> {
    let pattern = output.join(format!("page_%d.{to}"));
    let draw = args!["draw", "-q", "-r", "150", "-o", &pattern, input];
    run(app, job, sidecar(app, "mutool")?.args(draw), |_| {}).await
}

async fn run_calibre(
    app: &AppHandle,
    job: u32,
    input: &Path,
    output: &Path,
) -> Result<(), Failure> {
    let program = calibre_program(app).ok_or(Failure::Missing)?;
    // Calibre guarda su configuración aparte para no tocar la de un Calibre ya instalado
    let config = app
        .path()
        .app_local_data_dir()
        .map_err(|_| Failure::Failed)?
        .join("calibre");
    let command = app
        .shell()
        .command(program)
        .args(args![input, output])
        .env("CALIBRE_CONFIG_DIRECTORY", config);
    run(app, job, command, |line| {
        // Calibre informa con líneas como "34% Converting input to HTML..."
        let percent = line
            .split_once('%')
            .and_then(|(n, _)| n.trim().parse::<f64>().ok());
        if let Some(percent) = percent {
            emit_progress(app, job, percent / 100.0);
        }
    })
    .await
}

// Se extrae a una carpeta temporal y se vuelve a comprimir en el formato pedido
async fn run_archive(
    app: &AppHandle,
    job: u32,
    input: &Path,
    from: &str,
    to: &str,
    output: &Path,
    work: &Path,
) -> Result<(), Failure> {
    let files = work.join("files");
    let mut source = input.to_path_buf();
    if from == "tar.gz" {
        let unzipped = work.join("unzipped");
        extract(app, job, input, &unzipped).await?;
        source = fs::read_dir(&unzipped)
            .map_err(|e| io_failure(&e))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .next()
            .ok_or(Failure::Unreadable)?;
    }
    extract(app, job, &source, &files).await?;

    let (kind, target) = match to {
        // El .tar interior lleva el nombre del resultado: "fotos.tar.gz" contiene "fotos.tar"
        "tar.gz" => ("tar", work.join(output.file_stem().unwrap_or_default())),
        other => (other, output.to_path_buf()),
    };
    let pack = args!["a", "-bso0", "-bsp0", format!("-t{kind}"), &target, "*"];
    let command = sidecar(app, "7z")?.args(pack).current_dir(&files);
    run(app, job, command, |_| {}).await?;
    if to == "tar.gz" {
        let gzip = args!["a", "-bso0", "-bsp0", "-tgzip", output, &target];
        run(app, job, sidecar(app, "7z")?.args(gzip), |_| {}).await?;
    }
    Ok(())
}

async fn extract(app: &AppHandle, job: u32, archive: &Path, into: &Path) -> Result<(), Failure> {
    let mut target = OsString::from("-o");
    target.push(into);
    // "-p" sin contraseña hace que un archivo protegido falle en vez de quedarse esperando
    let list = args!["x", "-y", "-bso0", "-bsp0", "-p", target, archive];
    match run(app, job, sidecar(app, "7z")?.args(list), |_| {}).await {
        // Windows no deja crear enlaces simbólicos sin permisos especiales; el resto sí se
        // extrae y el enlace queda como un archivo vacío
        Err(Failure::Links) => Ok(()),
        other => other,
    }
}

async fn run_data(
    app: &AppHandle,
    job: u32,
    input: &Path,
    from: &str,
    to: &str,
    output: &Path,
) -> Result<(), Failure> {
    let (app, input, from, to, output) = (
        app.clone(),
        input.to_path_buf(),
        from.to_string(),
        to.to_string(),
        output.to_path_buf(),
    );
    tauri::async_runtime::spawn_blocking(move || {
        data::convert(&input, &from, &output, &to, |fraction| {
            emit_progress(&app, job, fraction);
            !app.state::<Jobs>().cancelled.lock().unwrap().contains(&job)
        })
    })
    .await
    .map_err(|_| Failure::Failed)?
}

async fn run(
    app: &AppHandle,
    job: u32,
    command: Command,
    mut on_line: impl FnMut(&str),
) -> Result<(), Failure> {
    let jobs = app.state::<Jobs>();
    let (mut events, child) = command.spawn().map_err(|_| Failure::Missing)?;
    {
        // Cancelar puede llegar entre dos pasos, antes de que exista el proceso
        if jobs.cancelled.lock().unwrap().contains(&job) {
            let _ = child.kill();
            return Err(Failure::Cancelled);
        }
        jobs.running.lock().unwrap().insert(job, child);
    }

    let mut errors = VecDeque::new();
    let mut code = None;
    while let Some(event) = events.recv().await {
        match event {
            CommandEvent::Stdout(line) => on_line(String::from_utf8_lossy(&line).trim()),
            CommandEvent::Stderr(line) => {
                errors.push_back(String::from_utf8_lossy(&line).trim().to_string());
                if errors.len() > 30 {
                    errors.pop_front();
                }
            }
            CommandEvent::Terminated(payload) => code = payload.code,
            _ => {}
        }
    }
    jobs.running.lock().unwrap().remove(&job);

    if jobs.cancelled.lock().unwrap().contains(&job) {
        return Err(Failure::Cancelled);
    }
    if code == Some(0) {
        return Ok(());
    }
    let errors = Vec::from(errors).join("\n");
    eprintln!("el motor terminó con {code:?}:\n{errors}");
    Err(classify(&errors))
}

fn classify(errors: &str) -> Failure {
    let text = errors.to_lowercase();
    let has = |hints: &[&str]| hints.iter().any(|hint| text.contains(hint));
    if has(&["no space left", "not enough space"]) {
        Failure::NoSpace
    } else if has(&["permission denied", "access is denied"]) {
        Failure::Denied
    } else if has(&["wrong password", "password", "encrypted", "drm"]) {
        Failure::Protected
    } else if has(&[
        "invalid data found",
        "not a known file format",
        "unable to load",
        "moov atom not found",
        "unsupported image format",
        "cannot open the file as",
        "cannot open document",
        "no such file",
        "could not load",
        "crc failed",
        "data error",
        "unexpected end",
    ]) {
        Failure::Unreadable
    } else if has(&["cannot create symbolic link"]) {
        Failure::Links
    } else {
        Failure::Failed
    }
}

pub fn io_failure(error: &std::io::Error) -> Failure {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => Failure::Denied,
        std::io::ErrorKind::StorageFull => Failure::NoSpace,
        _ => Failure::Failed,
    }
}
