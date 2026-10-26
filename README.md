# Swayfi

Conversor de archivos de escritorio. Arrastras un archivo, eliges el formato y listo: el resultado se guarda en tu carpeta **Descargas**.

## Descargar

[![Descargar para Windows](https://img.shields.io/badge/Descargar_para-Windows-0A78D8?style=for-the-badge&logo=windows11&logoColor=white)](https://github.com/garciaadov/swayfi/releases/latest/download/Swayfi-Windows-setup.exe)
[![Descargar para macOS](https://img.shields.io/badge/Descargar_para-macOS-8B12F2?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/garciaadov/swayfi/releases/latest/download/Swayfi-macOS.dmg)
[![Descargar para Linux](https://img.shields.io/badge/Descargar_para-Linux-0A3FE8?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/garciaadov/swayfi/releases/latest/download/Swayfi-Linux.deb)

- **Windows 10 u 11:** abre el archivo descargado y sigue el instalador.
- **macOS (Mac con chip M1 o posterior):** abre el `.dmg` y arrastra Swayfi a Aplicaciones. La primera vez hay que permitirla a mano: [mira cómo](#abrir-swayfi-en-macos-la-primera-vez).
- **Linux (Ubuntu, Debian y derivados):** instala el `.deb` con `sudo apt install ./Swayfi-Linux.deb`.

Más detalles en [Instalar](#instalar).

- **100 % offline.** Nada se sube a internet; todo ocurre en tu ordenador.
- **Sin límite de tamaño.** La interfaz solo maneja rutas; los motores leen y escriben en disco por partes.
- **Sin instalar nada más.** Todos los motores de conversión van dentro del instalador.

## Qué convierte

| Grupo | Si tu archivo es… | Lo puedes convertir a… |
| --- | --- | --- |
| Imágenes | JPG, PNG, GIF, WEBP, BMP, TIFF, HEIC, AVIF | Cualquiera de esa lista (salvo HEIC), ICO o PDF |
| Vectoriales | SVG | PNG, JPG, WEBP o PDF |
| Audio | MP3, WAV, FLAC, AAC, M4A, OGG, OPUS, WMA, AIFF | Cualquiera de esa lista |
| Vídeo | MP4, MKV, AVI, MOV, WEBM, FLV, WMV, MPEG, 3GP | Cualquiera de esa lista, audio, GIF o imágenes (un fotograma por segundo) |
| Documentos | DOCX, DOC, ODT, RTF, TXT, HTML, MD | Cualquiera de esa lista, PDF o EPUB |
| PDF | PDF | DOCX, TXT o imágenes (una por página) |
| Hojas de cálculo | XLSX, XLS, ODS, CSV, TSV | Cualquiera de esa lista, JSON o PDF |
| Datos | JSON, XML, YAML, CSV | Cualquiera de esa lista |
| Presentaciones | PPTX, PPT, ODP | Cualquiera de esa lista, PDF o imágenes (una por diapositiva) |
| Comprimidos | ZIP, RAR, 7Z, TAR, TAR.GZ | ZIP, 7Z, TAR o TAR.GZ |
| Libros electrónicos | EPUB, MOBI, AZW3, FB2 | Cualquiera de esa lista o PDF |

Las fuentes (TTF, OTF, WOFF, WOFF2) llegarán en una segunda fase.

## Instalar

Descarga el instalador de la última versión en la página de [Releases](../../releases).

- **Windows 10 u 11:** `Swayfi-Windows-setup.exe`. El instalador pesa unos 460 MB (unos 1,9 GB instalado) porque incluye LibreOffice y Calibre. Mientras no esté firmado, Windows mostrará el aviso de SmartScreen: pulsa «Más información» y «Ejecutar de todas formas».
- **Linux (Debian, Ubuntu 22.04 o posterior):** `Swayfi-Linux.deb`. Se instala con `sudo apt install ./Swayfi-Linux.deb`, que trae los motores como dependencias del sistema.
- **macOS 12 o posterior, Mac con Apple Silicon (M1 o posterior):** `Swayfi-macOS.dmg`. Abre el `.dmg` y arrastra Swayfi a Aplicaciones. Mira la sección siguiente antes de abrirla por primera vez.

### Abrir Swayfi en macOS la primera vez

Swayfi no está firmada con una cuenta de desarrollador de Apple (cuesta 99 €/año), así que macOS no la abre a la primera. No es un virus: el código es público en este repositorio. Para permitirla:

1. Abre Swayfi desde Aplicaciones. Aparecerá un aviso diciendo que Apple no ha podido verificarla: pulsa **OK** (o **Hecho**).
2. Abre **Ajustes del Sistema → Privacidad y seguridad**.
3. Baja hasta el mensaje «Se ha bloqueado el uso de "Swayfi"…» y pulsa **Abrir igualmente**.
4. Confirma con tu contraseña o Touch ID y pulsa **Abrir**.

Solo hace falta la primera vez. Si macOS dice que la app «está dañada», abre la app Terminal y ejecuta esto; después ábrela con normalidad:

```
xattr -dr com.apple.quarantine /Applications/Swayfi.app
```

## Cómo funciona

La interfaz (HTML, CSS y JavaScript sin frameworks, en `src/`) solo maneja rutas de archivo. Rust (`src-tauri/src/`) elige un motor según el formato de entrada y el de salida y lo lanza como proceso aparte:

| Motor | Para qué | Cómo se distribuye |
| --- | --- | --- |
| [FFmpeg](https://ffmpeg.org) | Audio, vídeo, GIF, fotogramas, leer HEIC | Sidecar |
| [libvips](https://www.libvips.org) | Imágenes, SVG, ICO | Sidecar + librerías |
| [Pandoc](https://pandoc.org) | DOCX, ODT, RTF, TXT, HTML, MD, EPUB | Sidecar |
| [7-Zip](https://www.7-zip.org) | Comprimidos | Sidecar (+ `7z.dll` en Windows) |
| [MuPDF](https://mupdf.com) (`mutool`) | PDF a texto e imágenes | Sidecar |
| [LibreOffice](https://www.libreoffice.org) | DOC, PDF, hojas de cálculo, presentaciones | Carpeta de recursos |
| [Calibre](https://calibre-ebook.com) | EPUB, MOBI, AZW3, FB2 | Carpeta de recursos |
| Rust (`serde_json`, `serde_yaml`, `quick-xml`, `csv`) | JSON, XML, YAML, CSV, TSV | Dentro de la app |

En Linux no se empaqueta ningún motor: el `.deb` los declara como dependencias y Swayfi usa los del sistema.

Si una conversión necesita dos motores, se encadenan (por ejemplo, MD → PDF pasa por Pandoc y luego por LibreOffice). Los pasos intermedios se hacen en una carpeta temporal dentro de la carpeta de destino y se borran al terminar.

```
src/
  index.html, style.css, app.js   interfaz, tabla de formatos y textos ES/EN
  logo.png
src-tauri/
  src/main.rs                     arranque y comandos que llama la interfaz
  src/convert.rs                  elección y ejecución de los motores
  src/data.rs                     JSON, XML, YAML, CSV y TSV en Rust
  tauri.conf.json                 configuración común
  tauri.windows.conf.json         DLL, LibreOffice y Calibre como recursos
  tauri.macos.conf.json           motores, LibreOffice.app y calibre.app como recursos
  tauri.linux.conf.json           dependencias del paquete .deb
  binaries/                       motores (no se suben al repositorio)
.github/workflows/release.yml     compila los instaladores
```

## Desarrollo

### Requisitos

- [Rust](https://rustup.rs) estable.
- Windows: [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) con «Desarrollo para el escritorio con C++». WebView2 ya viene con Windows 10 y 11.
- Linux: `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf` y los motores del sistema: `sudo apt install ffmpeg libvips-tools pandoc 7zip mupdf-tools libreoffice-writer-nogui libreoffice-calc-nogui libreoffice-impress-nogui calibre`.
- La CLI de Tauri: `cargo install tauri-cli --version "^2" --locked`.

### Motores en Windows

Los motores pesan unos 1,7 GB y no están en el repositorio. Los pasos exactos para descargarlos y colocarlos están en el paso «Descargar motores» de [`release.yml`](.github/workflows/release.yml); se pueden ejecutar tal cual en Git Bash desde la raíz del proyecto. El resultado debe quedar así:

```
src-tauri/binaries/
  ffmpeg-x86_64-pc-windows-msvc.exe
  vips-x86_64-pc-windows-msvc.exe
  pandoc-x86_64-pc-windows-msvc.exe
  7z-x86_64-pc-windows-msvc.exe
  mutool-x86_64-pc-windows-msvc.exe
  windows/
    *.dll, vips-modules-8.18/      DLL de vips y 7z.dll
    libreoffice/                   LibreOffice recortado
    calibre/                       Calibre completo
```

El sufijo es el *target triple* de Rust (`rustc -vV` lo muestra en la línea `host`). Si compilas con el toolchain GNU (`x86_64-pc-windows-gnu`), copia los mismos ejecutables con ese sufijo.

### Arrancar y compilar

```
cd src-tauri
cargo tauri dev      # abre la app con recarga automática de la interfaz
cargo tauri build    # genera el instalador en target/release/bundle/
```

La primera compilación tarda unos minutos porque copia LibreOffice y Calibre junto al ejecutable.

### Publicar una versión

Sube la versión en `src-tauri/tauri.conf.json` y `src-tauri/Cargo.toml`, crea una etiqueta y súbela:

```
git tag v0.2.0
git push origin v0.2.0
```

El workflow compila los instaladores de Windows, macOS y Linux y los adjunta a un borrador de release en GitHub. Revisa el borrador y publícalo.

Para actualizar un motor, cambia su versión en el bloque `env` del workflow. LibreOffice solo mantiene en `download.documentfoundation.org/libreoffice/stable/` las dos últimas versiones: si una descarga falla, probablemente toca actualizar `LIBREOFFICE_VERSION`.

## Limitaciones

- **HEIC solo se lee.** El codificador HEVC no se distribuye por sus patentes.
- **JSON, YAML y XML se cargan enteros en memoria.** CSV y TSV sí se procesan fila a fila, sin límite.
- **XLSX y XLS admiten como máximo 1.048.576 filas.** Es un límite del formato: un CSV más largo se recorta al convertirlo a hoja de cálculo.
- **Enlaces simbólicos dentro de comprimidos (Windows):** Windows no deja crearlos sin permisos especiales, así que el enlace queda como un archivo vacío; el resto del contenido se conserva.
- **RAR en Linux:** las distribuciones separan el soporte de RAR del 7-Zip libre. Para leer RAR instala `7zip-rar` (Debian/Ubuntu, repositorio *non-free*/*multiverse*).
- **macOS:** solo para Apple Silicon (los Mac con Intel no tienen instalador) y sin firmar, así que hay que permitirla a mano la primera vez. vips y MuPDF se toman de Homebrew durante la compilación y se empaquetan con sus librerías.
- **Firma:** los instaladores no están firmados. Como proyecto de código abierto, se puede pedir firma gratuita para Windows a [SignPath Foundation](https://signpath.org).

## Licencia

Swayfi se distribuye bajo la [GPL-3.0 o posterior](LICENSE).

Los instaladores incluyen programas de terceros sin modificar, cada uno con su licencia. El código fuente de cada uno está en su web oficial:

| Programa | Licencia | Código fuente |
| --- | --- | --- |
| FFmpeg (build GPL de BtbN) | GPL-3.0 | <https://ffmpeg.org/download.html> · <https://github.com/BtbN/FFmpeg-Builds> |
| libvips | LGPL-2.1 | <https://github.com/libvips/libvips> · <https://github.com/libvips/build-win64-mxe> |
| Pandoc | GPL-2.0 o posterior | <https://github.com/jgm/pandoc> |
| 7-Zip | LGPL-2.1 con restricción unRAR | <https://www.7-zip.org> · <https://github.com/ip7z/7zip> |
| MuPDF | AGPL-3.0 | <https://mupdf.com/releases> |
| LibreOffice | MPL-2.0 | <https://www.libreoffice.org/about-us/source-code/> |
| Calibre | GPL-3.0 | <https://github.com/kovidgoyal/calibre> |
