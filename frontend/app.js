const invoke = (command, args = {}) => window.__TAURI__.core.invoke(command, args);
const listen = (event, handler) => window.__TAURI__.event.listen(event, (payload) => handler(payload.payload));

const el = (id) => document.getElementById(id);

const ui = {
  subtitle: el('subtitle'),
  layoutBadge: el('layout-badge'),
  installDir: el('install-dir'),
  installHint: el('install-hint'),
  minRam: el('min-ram'),
  maxRam: el('max-ram'),
  minValue: el('min-value'),
  maxValue: el('max-value'),
  ramBadge: el('ram-badge'),
  ramHint: el('ram-hint'),
  repo: el('repo'),
  tag: el('tag'),
  updateBox: el('update-box'),
  javaPath: el('java-path'),
  javaHint: el('java-hint'),
  progressCard: el('progress-card'),
  progressStage: el('progress-stage'),
  progressPercent: el('progress-percent'),
  progressBar: el('progress-bar'),
  progressMessage: el('progress-message'),
  status: el('status'),
  btnCheck: el('btn-check'),
  btnBrowse: el('btn-browse'),
  btnJava: el('btn-java'),
  btnInstall: el('btn-install'),
  btnPlay: el('btn-play'),
  btnStop: el('btn-stop'),
};

let current = {
  settings: null,
  systemRamGb: 8,
  clientPresent: false,
  busy: false,
  running: false,
};

function setStatus(message, kind = '') {
  ui.status.textContent = message;
  ui.status.className = `status ${kind}`;
}

function setButtons() {
  ui.btnPlay.disabled = current.busy || !current.clientPresent || current.running;
  ui.btnInstall.disabled = current.busy;
  ui.btnCheck.disabled = current.busy;
  ui.btnBrowse.disabled = current.busy;
  ui.btnJava.disabled = current.busy;
  ui.btnStop.classList.toggle('hidden', !current.running);
}

function formatRam(value) {
  return `${value} GB`;
}

function clampSliders(changed) {
  let min = Number(ui.minRam.value);
  let max = Number(ui.maxRam.value);
  // Cap at the detected system memory and at a sane ceiling, so the track is usable.
  const systemCeiling = current.systemRamGb > 0 ? current.systemRamGb : 16;
  const ceiling = Math.max(2, Math.min(32, systemCeiling));

  ui.minRam.max = ceiling;
  ui.maxRam.max = ceiling;

  if (min > ceiling) {
    min = ceiling;
    ui.minRam.value = String(min);
  }
  if (max > ceiling) {
    max = ceiling;
    ui.maxRam.value = String(max);
  }

  if (changed === 'min' && min > max) {
    max = min;
    ui.maxRam.value = String(max);
  }
  if (changed === 'max' && max < min) {
    min = max;
    ui.minRam.value = String(min);
  }

  ui.minValue.textContent = formatRam(min);
  ui.maxValue.textContent = formatRam(max);
  ui.ramBadge.textContent = max === min ? formatRam(min) : `${min} – ${max} GB`;
  ui.ramBadge.className = 'badge';
}

function readSettings() {
  return {
    installDir: ui.installDir.value.trim(),
    minRamGb: Number(ui.minRam.value),
    maxRamGb: Number(ui.maxRam.value),
    javaPath: ui.javaPath.value.trim(),
    javaArgs: current.settings?.javaArgs ?? '',
    repo: ui.repo.value.trim(),
    installedTag: current.settings?.installedTag ?? '',
  };
}

const DEFAULT_MIN_RAM = 2;
const DEFAULT_MAX_RAM = 4;

function sanitizeRam(settings, systemRamGb) {
  const ceiling = Math.max(2, Math.min(32, systemRamGb > 0 ? systemRamGb : 8));
  let min = Number(settings.minRamGb);
  let max = Number(settings.maxRamGb);
  if (!Number.isFinite(min) || min < 1) min = DEFAULT_MIN_RAM;
  if (!Number.isFinite(max) || max < 1) max = DEFAULT_MAX_RAM;
  if (min > ceiling || max > ceiling) {
    min = Math.min(DEFAULT_MIN_RAM, ceiling);
    max = Math.min(DEFAULT_MAX_RAM, ceiling);
  }
  if (min > max) max = min;
  return { min, max };
}

function applySettings(settings) {
  current.settings = settings;
  const ram = sanitizeRam(settings, current.systemRamGb);
  ui.installDir.value = settings.installDir ?? '';
  ui.repo.value = settings.repo ?? '';
  ui.minRam.value = String(ram.min);
  ui.maxRam.value = String(ram.max);
  ui.javaPath.value = settings.javaPath ?? '';
  clampSliders();
}

function showProgress(stage, message, percent) {
  ui.progressCard.classList.remove('hidden');
  ui.progressStage.textContent = stage;
  ui.progressPercent.textContent = `${percent}%`;
  ui.progressBar.style.width = `${percent}%`;
  ui.progressMessage.textContent = message;
}

function hideProgress() {
  ui.progressCard.classList.add('hidden');
  ui.progressBar.style.width = '0%';
}

function renderUpdateBox(info) {
  ui.updateBox.classList.remove('hidden');
  if (info.updateAvailable) {
    ui.updateBox.className = 'update-box';
    ui.updateBox.innerHTML =
      `<strong>Update available: ${escapeHtml(info.tag)}</strong>\n` +
      `Asset: ${escapeHtml(info.asset.name)} (${humanSize(info.asset.size)})\n` +
      `Published: ${escapeHtml(info.publishedAt || 'unknown')}` +
      (info.notes ? `\n\n${escapeHtml(info.notes.slice(0, 400))}` : '');
  } else {
    ui.updateBox.className = 'update-box ok';
    ui.updateBox.innerHTML = `<strong>Up to date</strong> — ${escapeHtml(info.tag)} is already installed.`;
  }
}

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (character) => (
    { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[character]
  ));
}

function humanSize(bytes) {
  if (!bytes || bytes <= 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

async function refresh() {
  const info = await invoke('get_app_info');
  current.systemRamGb = info.systemRamGb || 8;
  current.clientPresent = info.clientPresent;
  current.running = info.running;

  applySettings(info.settings);

  ui.ramHint.textContent = `Detected system memory: ${info.systemRamGb} GB. Values above it are clamped.`;

  if (info.launcherScript) {
    ui.layoutBadge.textContent = 'bundle ready';
    ui.layoutBadge.className = 'badge';
    ui.installHint.textContent = `Launcher script: ${info.launcherScript}`;
  } else if (info.clientPresent) {
    ui.layoutBadge.textContent = 'classpath';
    ui.layoutBadge.className = 'badge';
    ui.installHint.textContent = 'Java client detected — it will be started directly.';
  } else {
    ui.layoutBadge.textContent = 'not installed';
    ui.layoutBadge.className = 'badge warn';
    ui.installHint.textContent = 'No client here yet. Press “Download client” to fetch it from GitHub.';
  }

  if (info.java.length) {
    const best = info.java[0];
    ui.javaHint.textContent = `${info.java.length} runtime(s) found. Newest: Java ${best.version}`;
    if (!ui.javaPath.value.trim()) ui.javaPath.placeholder = best.path;
  } else {
    ui.javaHint.textContent = 'No Java 17+ found. Install one or pick java.exe manually.';
  }

  setButtons();
}

async function checkForUpdates() {
  const repo = ui.repo.value.trim();
  if (!repo) {
    setStatus('Enter a repository as owner/repo', 'err');
    return;
  }
  current.busy = true;
  setButtons();
  setStatus('Checking GitHub…');
  try {
    const info = await invoke('check_update', { repo, tag: ui.tag.value.trim() });
    renderUpdateBox(info);
    setStatus(info.updateAvailable ? `Update ${info.tag} available` : 'Client is up to date', 'ok');
    if (info.updateAvailable) {
      ui.tag.value = info.tag;
    }
  } catch (error) {
    setStatus(String(error), 'err');
    ui.updateBox.classList.add('hidden');
  } finally {
    current.busy = false;
    setButtons();
  }
}

async function installClient() {
  const repo = ui.repo.value.trim();
  if (!repo) {
    setStatus('Enter a repository as owner/repo', 'err');
    return;
  }
  const target = ui.installDir.value.trim();
  if (!target) {
    setStatus('Choose an install folder first', 'err');
    return;
  }
  current.busy = true;
  setButtons();
  showProgress('Downloading', 'Connecting to GitHub…', 2);
  try {
    const settings = await invoke('download_and_install', {
      repo,
      tag: ui.tag.value.trim(),
      targetDir: target,
    });
    applySettings(settings);
    await refresh();
    showProgress('Done', `Installed ${settings.installedTag}`, 100);
    setStatus(`Client installed (${settings.installedTag})`, 'ok');
    setTimeout(hideProgress, 900);
  } catch (error) {
    hideProgress();
    setStatus(String(error), 'err');
  } finally {
    current.busy = false;
    setButtons();
  }
}

async function play() {
  current.busy = true;
  setButtons();
  setStatus('Starting the game…');
  try {
    const message = await invoke('launch_game', { settings: readSettings() });
    current.running = true;
    setStatus(message, 'ok');
  } catch (error) {
    setStatus(String(error), 'err');
  } finally {
    current.busy = false;
    setButtons();
  }
}

async function stop() {
  try {
    await invoke('stop_game');
    current.running = false;
    setStatus('Game stopped');
  } catch (error) {
    setStatus(String(error), 'err');
  }
  setButtons();
}

async function browse() {
  const picked = await invoke('pick_directory', { start: ui.installDir.value });
  if (picked) {
    ui.installDir.value = picked;
    await refresh();
  }
}

async function browseJava() {
  const picked = await invoke('pick_java');
  if (picked) ui.javaPath.value = picked;
}

function wire() {
  ui.btnCheck.addEventListener('click', checkForUpdates);
  ui.btnInstall.addEventListener('click', installClient);
  ui.btnPlay.addEventListener('click', play);
  ui.btnStop.addEventListener('click', stop);
  ui.btnBrowse.addEventListener('click', browse);
  ui.btnJava.addEventListener('click', browseJava);

  ui.minRam.addEventListener('input', () => clampSliders('min'));
  ui.maxRam.addEventListener('input', () => clampSliders('max'));

  for (const [input, key] of [[ui.installDir, 'installDir'], [ui.repo, 'repo'], [ui.javaPath, 'javaPath']]) {
    input.addEventListener('change', () => {
      if (!current.settings) return;
      current.settings[key] = input.value.trim();
      invoke('save_settings', { settings: current.settings }).catch(() => undefined);
    });
  }
  for (const input of [ui.minRam, ui.maxRam]) {
    input.addEventListener('change', () => {
      if (!current.settings) return;
      current.settings.minRamGb = Number(ui.minRam.value);
      current.settings.maxRamGb = Number(ui.maxRam.value);
      invoke('save_settings', { settings: current.settings }).catch(() => undefined);
    });
  }

  listen('install://progress', (payload) => {
    showProgress(payload.stage, payload.message, payload.percent);
    ui.subtitle.textContent = payload.message;
  });
}

async function poll() {
  try {
    const running = await invoke('is_game_running');
    if (running !== current.running) {
      current.running = running;
      setStatus(running ? 'Game is running' : 'Game closed', running ? 'ok' : '');
      setButtons();
    }
  } catch {
    // ignore transient errors while the game starts
  }
  setTimeout(poll, 1500);
}

async function start() {
  wire();
  try {
    await refresh();
    setStatus('Ready');
    if (ui.repo.value.trim()) {
      void checkForUpdates();
    } else {
      setStatus('Set the GitHub repository to enable updates');
    }
  } catch (error) {
    setStatus(String(error), 'err');
  }
  void poll();
}

window.addEventListener('DOMContentLoaded', start);
