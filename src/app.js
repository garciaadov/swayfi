const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.dialog;
const { revealItemInDir } = window.__TAURI__.opener;
const { getCurrentWebview } = window.__TAURI__.webview;

// Cada extensión pertenece a un tipo; el orden decide el de los grupos del desplegable
const KINDS = {
  video: ['mp4', 'mkv', 'avi', 'mov', 'webm', 'flv', 'wmv', 'mpeg', '3gp'],
  audio: ['mp3', 'wav', 'flac', 'aac', 'm4a', 'ogg', 'opus', 'wma', 'aiff'],
  image: ['jpg', 'png', 'gif', 'webp', 'bmp', 'tiff', 'heic', 'avif', 'ico', 'svg'],
  document: ['pdf', 'docx', 'doc', 'odt', 'rtf', 'txt', 'html', 'md'],
  spreadsheet: ['xlsx', 'xls', 'ods', 'csv', 'tsv'],
  data: ['json', 'xml', 'yaml'],
  presentation: ['pptx', 'ppt', 'odp'],
  ebook: ['epub', 'mobi', 'azw3', 'fb2'],
  archive: ['zip', '7z', 'tar', 'tar.gz', 'rar'],
};

const DOCUMENTS = ['docx', 'doc', 'odt', 'rtf', 'txt', 'html', 'md'];

// HEIC se puede leer pero no crear: el codificador HEVC no se distribuye por patentes.
// "folder" marca los destinos que generan varias imágenes en una carpeta.
// TODO: fuentes (TTF, OTF, WOFF, WOFF2) en una segunda fase.
const GROUPS = [
  {
    inputs: ['jpg', 'png', 'gif', 'webp', 'bmp', 'tiff', 'heic', 'avif'],
    outputs: ['jpg', 'png', 'gif', 'webp', 'bmp', 'tiff', 'avif', 'ico', 'pdf'],
  },
  { inputs: ['svg'], outputs: ['png', 'jpg', 'webp', 'pdf'] },
  { inputs: KINDS.audio, outputs: KINDS.audio },
  {
    inputs: KINDS.video,
    outputs: [...KINDS.video, ...KINDS.audio, 'gif', 'png', 'jpg'],
    folder: 'frames',
  },
  { inputs: DOCUMENTS, outputs: [...DOCUMENTS, 'pdf', 'epub'] },
  { inputs: ['pdf'], outputs: ['docx', 'txt', 'png', 'jpg'], folder: 'pages' },
  {
    inputs: ['xlsx', 'xls', 'ods', 'tsv'],
    outputs: ['xlsx', 'xls', 'ods', 'csv', 'tsv', 'json', 'pdf'],
  },
  // CSV es a la vez hoja de cálculo y datos: admite los destinos de los dos grupos
  { inputs: ['csv'], outputs: ['xlsx', 'xls', 'ods', 'tsv', 'json', 'xml', 'yaml', 'pdf'] },
  { inputs: ['json', 'xml', 'yaml'], outputs: ['json', 'xml', 'yaml', 'csv'] },
  { inputs: KINDS.presentation, outputs: [...KINDS.presentation, 'pdf', 'png', 'jpg'], folder: 'slides' },
  { inputs: ['zip', 'rar', '7z', 'tar', 'tar.gz'], outputs: ['zip', '7z', 'tar', 'tar.gz'] },
  { inputs: KINDS.ebook, outputs: [...KINDS.ebook, 'pdf'] },
];

const TEXTS = {
  es: {
    help: 'Ayuda',
    theme: 'Cambiar tema',
    language: 'Idioma',
    helpTitle: 'Cómo funciona',
    help1: 'Arrastra uno o varios archivos, o haz clic para elegirlos.',
    help2: 'Elige el formato. De un vídeo también puedes sacar el audio, un GIF o un fotograma por segundo.',
    help3: 'Pulsa Convertir. El resultado se guarda en tu carpeta Descargas, salvo que elijas otra.',
    helpPrivacy: 'Todo ocurre en tu ordenador: nada se sube a internet.',
    dropTitle: 'Arrastra archivos aquí o haz clic para elegirlos',
    dropHint: 'Imágenes, audio, vídeo, documentos y más · Sin límite de tamaño · 100 % offline',
    noneSupported: 'Swayfi todavía no sabe convertir este tipo de archivo.',
    back: 'Otro archivo',
    convert: 'Convertir',
    to: 'a',
    files: '{n} archivos',
    folder_frames: 'fotogramas',
    folder_pages: 'páginas',
    folder_slides: 'diapositivas',
    install_libreoffice: 'Instala LibreOffice para activar esto',
    install_calibre: 'Instala Calibre para activar esto',
    skipped: 'Se omitirán por no ser compatibles: {names}',
    saveTo: 'Se guardará',
    defaultFolder: 'en Descargas',
    changeFolder: 'Cambiar carpeta',
    converting: 'Convirtiendo {name}…',
    convertingMany: 'Convirtiendo {name} ({i} de {n})…',
    cancel: 'Cancelar',
    cancelled: 'Conversión cancelada.',
    doneOne: 'Listo. Se ha guardado {name}.',
    doneMany: 'Listo. Se han convertido {n} archivos.',
    someFailed: 'Se han convertido {ok} de {n} archivos.',
    failedFile: 'No se pudo convertir {name}. {reason}',
    openFolder: 'Abrir carpeta',
    kind_video: 'Vídeo',
    kind_audio: 'Audio',
    kind_image: 'Imagen',
    kind_document: 'Documento',
    kind_spreadsheet: 'Hoja de cálculo',
    kind_data: 'Datos',
    kind_presentation: 'Presentación',
    kind_ebook: 'Libro electrónico',
    kind_archive: 'Comprimido',
    error_unsupported: 'Swayfi no sabe hacer esta conversión.',
    error_missing: 'Falta una parte de Swayfi. Vuelve a instalar la aplicación.',
    error_unreadable: 'El archivo parece dañado o no es del tipo que indica su extensión.',
    error_protected: 'El archivo está protegido con contraseña o con DRM.',
    error_nospace: 'No queda espacio en el disco. Libera espacio o elige otra carpeta.',
    error_denied: 'No hay permiso para guardar ahí. Prueba con «Cambiar carpeta».',
    error_failed: 'Algo ha fallado durante la conversión. Prueba con otro formato.',
  },
  en: {
    help: 'Help',
    theme: 'Switch theme',
    language: 'Language',
    helpTitle: 'How it works',
    help1: 'Drop one or more files, or click to choose them.',
    help2: 'Pick a format. From a video you can also get the audio, a GIF or one frame per second.',
    help3: 'Press Convert. The result is saved to your Downloads folder unless you choose another one.',
    helpPrivacy: 'Everything happens on your computer: nothing is uploaded.',
    dropTitle: 'Drop files here or click to choose them',
    dropHint: 'Images, audio, video, documents and more · No size limit · 100% offline',
    noneSupported: 'Swayfi can’t convert this kind of file yet.',
    back: 'Other file',
    convert: 'Convert',
    to: 'to',
    files: '{n} files',
    folder_frames: 'frames',
    folder_pages: 'pages',
    folder_slides: 'slides',
    install_libreoffice: 'Install LibreOffice to enable this',
    install_calibre: 'Install Calibre to enable this',
    skipped: 'These will be skipped because they aren’t supported: {names}',
    saveTo: 'Saving',
    defaultFolder: 'to Downloads',
    changeFolder: 'Change folder',
    converting: 'Converting {name}…',
    convertingMany: 'Converting {name} ({i} of {n})…',
    cancel: 'Cancel',
    cancelled: 'Conversion cancelled.',
    doneOne: 'Done. Saved {name}.',
    doneMany: 'Done. {n} files converted.',
    someFailed: '{ok} of {n} files converted.',
    failedFile: 'Couldn’t convert {name}. {reason}',
    openFolder: 'Open folder',
    kind_video: 'Video',
    kind_audio: 'Audio',
    kind_image: 'Image',
    kind_document: 'Document',
    kind_spreadsheet: 'Spreadsheet',
    kind_data: 'Data',
    kind_presentation: 'Presentation',
    kind_ebook: 'E-book',
    kind_archive: 'Archive',
    error_unsupported: 'Swayfi can’t do this conversion.',
    error_missing: 'Part of Swayfi is missing. Please reinstall the app.',
    error_unreadable: 'The file looks damaged or isn’t the type its extension says.',
    error_protected: 'The file is protected by a password or DRM.',
    error_nospace: 'The disk is full. Free up some space or choose another folder.',
    error_denied: 'No permission to save there. Try “Change folder”.',
    error_failed: 'Something went wrong during the conversion. Try another format.',
  },
};

const $ = (selector) => document.querySelector(selector);
const darkQuery = matchMedia('(prefers-color-scheme: dark)');

let lang = localStorage.getItem('lang') || (navigator.language.startsWith('es') ? 'es' : 'en');
let entries = [];
let skipped = [];
let outDir = null;
let busy = false;
let currentJob = null;
let nextJob = 1;
let cancelRequested = false;
let lastOutput = null;
let progressStep = { index: 0, total: 1 };

function t(key, vars = {}) {
  return TEXTS[lang][key].replace(/\{(\w+)\}/g, (_, name) => vars[name]);
}

function applyTexts() {
  document.documentElement.lang = lang;
  $('#lang-select').value = lang;
  for (const el of document.querySelectorAll('[data-i18n]')) {
    el.textContent = t(el.dataset.i18n);
  }
  for (const el of document.querySelectorAll('[data-i18n-label]')) {
    el.title = t(el.dataset.i18nLabel);
    el.setAttribute('aria-label', el.title);
  }
}

function toggleTheme() {
  const current = document.documentElement.dataset.theme || (darkQuery.matches ? 'dark' : 'light');
  const next = current === 'dark' ? 'light' : 'dark';
  document.documentElement.dataset.theme = next;
  localStorage.setItem('theme', next);
}

function kindOf(ext) {
  return Object.keys(KINDS).find((kind) => KINDS[kind].includes(ext));
}

function formatSize(bytes) {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (bytes >= 1024 && i < units.length - 1) {
    bytes /= 1024;
    i++;
  }
  return `${bytes.toLocaleString(lang, { maximumFractionDigits: i ? 1 : 0 })} ${units[i]}`;
}

function fileName(path) {
  return path.split(/[\\/]/).pop();
}

function showScreen(name) {
  $('#home').hidden = name !== 'home';
  $('#convert').hidden = name !== 'convert';
}

async function loadFiles(paths) {
  if (!paths || paths.length === 0) return;
  const infos = await invoke('file_info', { paths });

  entries = [];
  skipped = [];
  for (const file of infos) {
    const group = GROUPS.find((g) => g.inputs.includes(file.ext));
    if (!group) {
      skipped.push(file);
      continue;
    }
    let entry = entries.find((e) => e.group === group);
    if (!entry) {
      entry = { group, files: [], target: null, missing: {} };
      entries.push(entry);
    }
    entry.files.push(file);
  }

  const homeError = $('#home-error');
  homeError.hidden = entries.length > 0;
  if (entries.length === 0) {
    homeError.textContent = t('noneSupported');
    return;
  }

  // LibreOffice y Calibre van dentro del instalador, pero en desarrollo pueden faltar
  for (const entry of entries) {
    const exts = [...new Set(entry.files.map((f) => f.ext))];
    entry.missing = await invoke('missing_tools', { exts, targets: entry.group.outputs });
    entry.target = targetsFor(entry).find((ext) => !entry.missing[ext]);
  }
  outDir = null;
  $('#progress').hidden = true;
  $('#result').hidden = true;
  renderConvert();
  showScreen('convert');
}

// Si todos los archivos ya están en un formato, no tiene sentido ofrecerlo como destino
function targetsFor(entry) {
  const exts = new Set(entry.files.map((f) => f.ext));
  return entry.group.outputs.filter((ext) => !(exts.size === 1 && exts.has(ext)));
}

function renderConvert() {
  $('#entries').replaceChildren(...entries.map(renderEntry));
  $('#skipped').hidden = skipped.length === 0;
  $('#skipped').textContent = t('skipped', { names: skipped.map((f) => f.name).join(', ') });
  $('#destination').textContent = outDir || t('defaultFolder');
  $('#destination').title = outDir || '';
}

function renderEntry(entry) {
  const name = document.createElement('strong');
  name.textContent =
    entry.files.length === 1 ? entry.files[0].name : t('files', { n: entry.files.length });
  name.title = entry.files.map((f) => f.name).join('\n');

  const select = document.createElement('select');
  fillSelect(select, entry);
  select.value = entry.target;
  select.disabled = busy;
  select.addEventListener('change', () => {
    entry.target = select.value;
    $('#result').hidden = true;
  });

  const sentence = document.createElement('p');
  sentence.className = 'sentence';
  sentence.append(t('convert'), name, t('to'), select);

  const list = document.createElement('ul');
  list.className = 'files';
  for (const file of entry.files) {
    const item = document.createElement('li');
    item.textContent = `${file.name} · ${t('kind_' + kindOf(file.ext))} · ${formatSize(file.size)}`;
    list.append(item);
  }

  const card = document.createElement('div');
  card.className = 'card';
  card.append(sentence, list);
  return card;
}

function fillSelect(select, entry) {
  const targets = targetsFor(entry);
  // Primero los formatos del mismo tipo que el archivo; luego el resto
  const own = kindOf(entry.files[0].ext);
  for (const kind of [own, ...Object.keys(KINDS).filter((k) => k !== own)]) {
    const exts = targets.filter((ext) => kindOf(ext) === kind);
    if (exts.length === 0) continue;
    const optgroup = document.createElement('optgroup');
    optgroup.label = t('kind_' + kind);
    for (const ext of exts) {
      let label = ext.toUpperCase();
      if (entry.group.folder && (ext === 'png' || ext === 'jpg')) {
        label += ` (${t('folder_' + entry.group.folder)})`;
      }
      const tool = entry.missing[ext];
      if (tool) label += ` · ${t('install_' + tool)}`;
      const option = new Option(label, ext);
      option.disabled = Boolean(tool);
      optgroup.append(option);
    }
    select.append(optgroup);
  }
}

async function chooseFolder() {
  const dir = await open({ directory: true });
  if (dir) {
    outDir = dir;
    renderConvert();
  }
}

function setBusy(value) {
  busy = value;
  for (const el of document.querySelectorAll('#convert select, #convert-button, #back-button, #folder-button')) {
    el.disabled = value;
  }
  $('#progress').hidden = !value;
  if (value) $('#result').hidden = true;
}

function setBar(fraction) {
  const fill = $('#bar-fill');
  fill.classList.remove('waiting');
  fill.style.width = `${fraction * 100}%`;
  $('#progress-percent').textContent = `${Math.floor(fraction * 100)} %`;
}

function startStep(index, total, name) {
  progressStep = { index, total };
  $('#progress-text').textContent =
    total === 1 ? t('converting', { name }) : t('convertingMany', { name, i: index + 1, n: total });
  // Solo FFmpeg, Calibre y los CSV informan del avance; los demás muestran la barra sin porcentaje
  if (total === 1) {
    $('#bar-fill').classList.add('waiting');
    $('#progress-percent').textContent = '';
  } else {
    setBar(index / total);
  }
}

async function convertAll() {
  const queue = entries
    .filter((entry) => entry.target)
    .flatMap((entry) => entry.files.map((file) => ({ file, target: entry.target })));
  const failures = [];
  let converted = 0;
  cancelRequested = false;
  lastOutput = null;
  setBusy(true);

  for (const [index, { file, target }] of queue.entries()) {
    if (cancelRequested) break;
    currentJob = nextJob++;
    startStep(index, queue.length, file.name);
    try {
      lastOutput = await invoke('convert_file', { job: currentJob, path: file.path, target, outDir });
      converted++;
    } catch (code) {
      if (code !== 'cancelled') {
        failures.push(t('failedFile', { name: file.name, reason: t('error_' + code) }));
      }
    }
  }

  currentJob = null;
  setBusy(false);
  showResult(converted, queue.length, failures);
}

function showResult(converted, total, failures) {
  const lines = [];
  if (cancelRequested) lines.push(t('cancelled'));
  else if (failures.length === 0 && total === 1) lines.push(t('doneOne', { name: fileName(lastOutput) }));
  else if (failures.length === 0) lines.push(t('doneMany', { n: converted }));
  else if (converted > 0) lines.push(t('someFailed', { ok: converted, n: total }));
  lines.push(...failures);

  const text = $('#result-text');
  text.textContent = lines.join('\n');
  text.className = `message ${failures.length || cancelRequested ? 'error' : 'success'}`;
  $('#open-button').hidden = !lastOutput;
  $('#result').hidden = false;
}

function cancelConversion() {
  cancelRequested = true;
  if (currentJob) invoke('cancel_job', { job: currentJob });
}

function toggleHelp(show) {
  $('#help-panel').hidden = !show;
  $('#help-button').setAttribute('aria-expanded', String(show));
}

if (localStorage.getItem('theme')) {
  document.documentElement.dataset.theme = localStorage.getItem('theme');
}
applyTexts();

listen('progress', ({ payload }) => {
  if (payload.job === currentJob) {
    setBar((progressStep.index + payload.fraction) / progressStep.total);
  }
});

getCurrentWebview().onDragDropEvent(({ payload }) => {
  const dropzone = $('#dropzone');
  if (busy) return;
  if (payload.type === 'enter' || payload.type === 'over') {
    dropzone.classList.add('over');
  } else {
    dropzone.classList.remove('over');
    if (payload.type === 'drop') loadFiles(payload.paths);
  }
});

$('#dropzone').addEventListener('click', async () => {
  loadFiles(await open({ multiple: true }));
});

$('#help-button').addEventListener('click', (event) => {
  event.stopPropagation();
  toggleHelp($('#help-panel').hidden);
});

document.addEventListener('click', (event) => {
  if (!$('#help-panel').contains(event.target)) toggleHelp(false);
});

document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') toggleHelp(false);
});

$('#theme-button').addEventListener('click', toggleTheme);

$('#lang-select').addEventListener('change', (event) => {
  lang = event.target.value;
  localStorage.setItem('lang', lang);
  applyTexts();
  if (entries.length) renderConvert();
});

$('#back-button').addEventListener('click', () => {
  entries = [];
  showScreen('home');
});

$('#folder-button').addEventListener('click', chooseFolder);
$('#convert-button').addEventListener('click', convertAll);
$('#cancel-button').addEventListener('click', cancelConversion);
$('#open-button').addEventListener('click', () => revealItemInDir(lastOutput));
