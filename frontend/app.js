const invoke = (command, args = {}) => window.__TAURI__.core.invoke(command, args);
const listen = (event, handler) => window.__TAURI__.event.listen(event, (payload) => handler(payload.payload));
const el = (id) => document.getElementById(id);

const ui = {
  subtitle: el('subtitle'),
  tabs: Array.from(document.querySelectorAll('.tab')),
  panels: {
    play: el('tab-play'),
    mods: el('tab-mods'),
    settings: el('tab-settings'),
  },
  layoutBadge: el('layout-badge'),
  heroTitle: el('hero-title'),
  heroPath: el('hero-path'),
  repo: el('repo'),
  tag: el('tag'),
  updateBox: el('update-box'),
  progressCard: el('progress-card'),
  progressStage: el('progress-stage'),
  progressPercent: el('progress-percent'),
  progressBar: el('progress-bar'),
  progressMessage: el('progress-message'),
  installDir: el('install-dir'),
  installHint: el('install-hint'),
  minRam: el('min-ram'),
  maxRam: el('max-ram'),
  minValue: el('min-value'),
  maxValue: el('max-value'),
  ramBadge: el('ram-badge'),
  ramHint: el('ram-hint'),
  javaPath: el('java-path'),
  javaHint: el('java-hint'),
  javaArgs: el('java-args'),
  modsPath: el('mods-path'),
  modsList: el('mods-list'),
  modQuery: el('mod-query'),
  modResults: el('mod-results'),
  status: el('status'),
  btnCheck: el('btn-check'),
  btnInstall: el('btn-install'),
  btnPlay: el('btn-play'),
  btnStop: el('btn-stop'),
  btnBrowse: el('btn-browse'),
  btnJava: el('btn-java'),
  btnAddMod: el('btn-add-mod'),
  btnOpenMods: el('btn-open-mods'),
  btnSearchMods: el('btn-search-mods'),
};

const DEFAULT_MIN_RAM = 2;
const DEFAULT_MAX_RAM = 4;

const state = {
  settings: null,
  systemRamGb: 8,
  clientPresent: false,
  busy: false,
  running: false,
  mods: [],
};

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (character) => (
    { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[character]
  ));
}

function setStatus(message, kind = '') {
  ui.status.textContent = message;
  ui.status.className = `status ${kind}`;
}

function humanSize(bytes) {
  if (!bytes || bytes <= 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit += 1; }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function setButtons() {
  const missing = !state.clientPresent;
  ui.btnPlay.disabled = state.busy || missing || state.running;
  ui.btnInstall.disabled = state.busy;
  ui.btnCheck.disabled = state.busy;
  ui.btnBrowse.disabled = state.busy;
  ui.btnJava.disabled = state.busy;
  ui.btnAddMod.disabled = state.busy || missing;
  ui.btnSearchMods.disabled = state.busy;
  ui.btnStop.classList.toggle('hidden', !state.running);
}

/* ---------- Tabs ---------- */
function switchTab(name) {
  for (const tab of ui.tabs) tab.classList.toggle('active', tab.dataset.tab === name);
  for (const [key, panel] of Object.entries(ui.panels)) panel.classList.toggle('hidden', key !== name);
  if (name === 'mods') void loadMods();
}

/* ---------- RAM ---------- */
function clampSliders(changed) {
  let min = Number(ui.minRam.value);
  let max = Number(ui.maxRam.value);
  const systemCeiling = state.systemRamGb > 0 ? state.systemRamGb : 16;
  const ceiling = Math.max(2, Math.min(64, systemCeiling));
  ui.minRam.max = ceiling;
  ui.maxRam.max = ceiling;
  if (min > ceiling) { min = ceiling; ui.minRam.value = String(min); }
  if (max > ceiling) { max = ceiling; ui.maxRam.value = String(max); }
  if (changed === 'min' && min > max) { max = min; ui.maxRam.value = String(max); }
  if (changed === 'max' && max < min) { min = max; ui.minRam.value = String(min); }
  ui.minValue.textContent = `${min} GB`;
  ui.maxValue.textContent = `${max} GB`;
  ui.ramBadge.textContent = max === min ? `${min} GB` : `${min} – ${max} GB`;
  ui.ramBadge.className = 'badge';
}

function sanitizeRam(settings) {
  const ceiling = Math.max(2, Math.min(64, state.systemRamGb > 0 ? state.systemRamGb : 8));
  let min = Number(settings.minRamGb);
  let max = Number(settings.maxRamGb);
  if (!Number.isFinite(min) || min < 1 || min > ceiling) min = Math.min(DEFAULT_MIN_RAM, ceiling);
  if (!Number.isFinite(max) || max < 1 || max > ceiling) max = Math.min(DEFAULT_MAX_RAM, ceiling);
  if (min > max) max = min;
  return { min, max };
}

function readSettings() {
  return {
    installDir: ui.installDir.value.trim(),
    minRamGb: Number(ui.minRam.value),
    maxRamGb: Number(ui.maxRam.value),
    javaPath: ui.javaPath.value.trim(),
    javaArgs: ui.javaArgs.value.trim(),
    repo: ui.repo.value.trim(),
    installedTag: state.settings?.installedTag ?? '',
  };
}

function applySettings(settings) {
  state.settings = settings;
  const ram = sanitizeRam(settings);
  ui.installDir.value = settings.installDir ?? '';
  ui.repo.value = settings.repo ?? '';
  ui.javaPath.value = settings.javaPath ?? '';
  ui.javaArgs.value = settings.javaArgs ?? '';
  ui.minRam.value = String(ram.min);
  ui.maxRam.value = String(ram.max);
  clampSliders();
}

async function persist() {
  if (!state.settings) return;
  const next = readSettings();
  state.settings = { ...state.settings, ...next };
  await invoke('save_settings', { settings: state.settings }).catch(() => undefined);
}

/* ---------- Progress ---------- */
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

/* ---------- Refresh ---------- */
async function refresh() {
  const info = await invoke('get_app_info');
  state.systemRamGb = info.systemRamGb || 8;
  state.clientPresent = info.clientPresent;
  state.running = info.running;
  applySettings(info.settings);

  ui.ramHint.textContent = `Detected system memory: ${info.systemRamGb} GB. Values above it are clamped.`;

  if (info.launcherScript) {
    ui.layoutBadge.textContent = 'ready';
    ui.layoutBadge.className = 'badge';
    ui.heroTitle.textContent = 'Your client is installed';
    ui.heroPath.textContent = info.launcherScript;
    ui.installHint.textContent = `Launcher script: ${info.launcherScript}`;
  } else if (info.clientPresent) {
    ui.layoutBadge.textContent = 'java client';
    ui.layoutBadge.className = 'badge';
    ui.heroTitle.textContent = 'Java client detected';
    ui.heroPath.textContent = info.settings.installDir;
    ui.installHint.textContent = 'The client will be started through Java.';
  } else {
    ui.layoutBadge.textContent = 'not installed';
    ui.layoutBadge.className = 'badge warn';
    ui.heroTitle.textContent = 'Install a client to get started';
    ui.heroPath.textContent = info.settings.installDir || '—';
    ui.installHint.textContent = 'No client here yet. Press “Download client” to fetch it from GitHub.';
  }

  if (info.java.length) {
    ui.javaHint.textContent = `${info.java.length} runtime(s) found. Newest: Java ${info.java[0].version}`;
    if (!ui.javaPath.value.trim()) ui.javaPath.placeholder = info.java[0].path;
  } else {
    ui.javaHint.textContent = 'No Java 17+ found. Install one or pick java.exe manually.';
  }

  setButtons();
}

/* ---------- Updates ---------- */
async function checkForUpdates() {
  const repo = ui.repo.value.trim();
  if (!repo) { setStatus('Enter a repository as owner/repo', 'err'); return; }
  state.busy = true; setButtons();
  setStatus('Checking GitHub…');
  try {
    const info = await invoke('check_update', { repo, tag: ui.tag.value.trim() });
    ui.updateBox.classList.remove('hidden');
    if (info.updateAvailable) {
      ui.updateBox.className = 'update-box';
      ui.updateBox.innerHTML =
        `<strong>Update available: ${escapeHtml(info.tag)}</strong><br>` +
        `${escapeHtml(info.asset.name)} · ${humanSize(info.asset.size)}` +
        (info.notes ? `<br><br>${escapeHtml(info.notes.slice(0, 400))}` : '');
      setStatus(`Update ${info.tag} is available`, 'ok');
      ui.tag.value = info.tag;
    } else {
      ui.updateBox.className = 'update-box ok';
      ui.updateBox.innerHTML = `<strong>Up to date</strong> — ${escapeHtml(info.tag)} is installed.`;
      setStatus('Client is up to date', 'ok');
    }
  } catch (error) {
    ui.updateBox.classList.add('hidden');
    setStatus(String(error), 'err');
  } finally {
    state.busy = false; setButtons();
  }
}

async function installClient() {
  const repo = ui.repo.value.trim();
  if (!repo) { setStatus('Enter a repository as owner/repo', 'err'); return; }
  const target = ui.installDir.value.trim();
  if (!target) { switchTab('settings'); setStatus('Choose an install folder first', 'err'); return; }
  state.busy = true; setButtons();
  showProgress('Downloading', 'Connecting to GitHub…', 2);
  try {
    const settings = await invoke('download_and_install', { repo, tag: ui.tag.value.trim(), targetDir: target });
    applySettings(settings);
    await refresh();
    showProgress('Done', `Installed ${settings.installedTag}`, 100);
    setStatus(`Client installed (${settings.installedTag})`, 'ok');
    setTimeout(hideProgress, 900);
  } catch (error) {
    hideProgress();
    setStatus(String(error), 'err');
  } finally {
    state.busy = false; setButtons();
  }
}

/* ---------- Play ---------- */
async function play() {
  state.busy = true; setButtons();
  setStatus('Starting the game…');
  try {
    const message = await invoke('launch_game', { settings: readSettings() });
    state.running = true;
    setStatus(message, 'ok');
  } catch (error) {
    setStatus(String(error), 'err');
  } finally {
    state.busy = false; setButtons();
  }
}

async function stop() {
  try {
    await invoke('stop_game');
    state.running = false;
    setStatus('Game stopped');
  } catch (error) {
    setStatus(String(error), 'err');
  }
  setButtons();
}

/* ---------- Dialogs ---------- */
async function browse() {
  setStatus('Opening the folder picker…');
  try {
    const picked = await invoke('pick_directory', { start: ui.installDir.value });
    if (picked) {
      ui.installDir.value = picked;
      await persist();
      await refresh();
      setStatus(`Install folder set to ${picked}`, 'ok');
    } else {
      setStatus('Folder selection cancelled');
    }
  } catch (error) {
    setStatus(`Could not open the folder picker: ${error}`, 'err');
  }
}

async function browseJava() {
  try {
    const picked = await invoke('pick_java');
    if (picked) {
      ui.javaPath.value = picked;
      await persist();
      setStatus('Java path updated', 'ok');
    }
  } catch (error) {
    setStatus(`Could not open the file picker: ${error}`, 'err');
  }
}

/* ---------- Mods ---------- */
function renderMods() {
  if (!state.mods.length) {
    ui.modsList.innerHTML = '<div class="empty">No mods installed yet.</div>';
    return;
  }
  ui.modsList.innerHTML = state.mods.map((mod, index) => `
    <div class="mod-row ${mod.enabled ? '' : 'off'}">
      <div class="switch ${mod.enabled ? 'on' : ''}" data-toggle="${index}" role="switch" aria-checked="${mod.enabled}"></div>
      <span class="mod-name" title="${escapeHtml(mod.fileName)}">${escapeHtml(mod.fileName)}</span>
      <span class="mod-size">${humanSize(mod.size)}</span>
      <button class="btn btn-danger" data-remove="${index}">Delete</button>
    </div>`).join('');
}

async function loadMods() {
  try {
    ui.modsPath.textContent = await invoke('mods_dir');
    state.mods = await invoke('list_mods');
    renderMods();
  } catch (error) {
    ui.modsPath.textContent = '—';
    ui.modsList.innerHTML = `<div class="empty">${escapeHtml(String(error))}</div>`;
  }
}

async function toggleMod(index) {
  const mod = state.mods[index];
  if (!mod) return;
  try {
    await invoke('set_mod_enabled', { fileName: mod.fileName, enabled: !mod.enabled });
    await loadMods();
    setStatus(`${mod.fileName} ${mod.enabled ? 'disabled' : 'enabled'}`, 'ok');
  } catch (error) {
    setStatus(String(error), 'err');
  }
}

async function removeMod(index) {
  const mod = state.mods[index];
  if (!mod) return;
  try {
    await invoke('remove_mod', { fileName: mod.fileName });
    await loadMods();
    setStatus(`${mod.fileName} deleted`, 'ok');
  } catch (error) {
    setStatus(String(error), 'err');
  }
}

async function addMod() {
  try {
    const added = await invoke('pick_mod_file');
    if (added) {
      await loadMods();
      const count = String(added).split('\n').filter(Boolean).length;
      setStatus(`${count} mod file(s) added`, 'ok');
    }
  } catch (error) {
    setStatus(String(error), 'err');
  }
}

async function openModsFolder() {
  try {
    await invoke('open_mods_folder');
  } catch (error) {
    setStatus(String(error), 'err');
  }
}

async function searchMods() {
  const query = ui.modQuery.value.trim();
  if (!query) return;
  ui.modResults.innerHTML = '<div class="empty">Searching…</div>';
  try {
    const hits = await invoke('search_mods', { query, gameVersion: '' });
    if (!hits.length) {
      ui.modResults.innerHTML = '<div class="empty">Nothing found.</div>';
      return;
    }
    ui.modResults.innerHTML = hits.map((hit, index) => `
      <div class="mod-hit">
        ${hit.iconUrl ? `<img src="${escapeHtml(hit.iconUrl)}" alt="" loading="lazy" />` : ''}
        <div class="mod-hit-info">
          <strong>${escapeHtml(hit.title)}</strong>
          <span>${escapeHtml(hit.description)}</span>
        </div>
        <button class="btn btn-primary" data-install="${index}">Install</button>
      </div>`).join('');
    ui.modResults.dataset.hits = JSON.stringify(hits);
  } catch (error) {
    ui.modResults.innerHTML = `<div class="empty">${escapeHtml(String(error))}</div>`;
  }
}

async function installModrinthMod(index) {
  let hits = [];
  try { hits = JSON.parse(ui.modResults.dataset.hits || '[]'); } catch { hits = []; }
  const hit = hits[index];
  if (!hit) return;
  setStatus(`Installing ${hit.title}…`);
  try {
    const name = await invoke('install_mod_from_modrinth', { projectId: hit.id, gameVersion: '' });
    await loadMods();
    setStatus(`Installed ${name}`, 'ok');
  } catch (error) {
    setStatus(String(error), 'err');
  }
}

/* ---------- Wiring ---------- */
function wire() {
  for (const tab of ui.tabs) tab.addEventListener('click', () => switchTab(tab.dataset.tab));

  ui.btnCheck.addEventListener('click', checkForUpdates);
  ui.btnInstall.addEventListener('click', installClient);
  ui.btnPlay.addEventListener('click', play);
  ui.btnStop.addEventListener('click', stop);
  ui.btnBrowse.addEventListener('click', browse);
  ui.btnJava.addEventListener('click', browseJava);
  ui.btnAddMod.addEventListener('click', addMod);
  ui.btnOpenMods.addEventListener('click', openModsFolder);
  ui.btnSearchMods.addEventListener('click', searchMods);

  ui.modQuery.addEventListener('keydown', (event) => { if (event.key === 'Enter') void searchMods(); });

  ui.modsList.addEventListener('click', (event) => {
    const toggle = event.target.closest('[data-toggle]');
    if (toggle) { void toggleMod(Number(toggle.dataset.toggle)); return; }
    const remove = event.target.closest('[data-remove]');
    if (remove) void removeMod(Number(remove.dataset.remove));
  });
  ui.modResults.addEventListener('click', (event) => {
    const install = event.target.closest('[data-install]');
    if (install) void installModrinthMod(Number(install.dataset.install));
  });

  ui.minRam.addEventListener('input', () => clampSliders('min'));
  ui.maxRam.addEventListener('input', () => clampSliders('max'));
  ui.minRam.addEventListener('change', () => void persist());
  ui.maxRam.addEventListener('change', () => void persist());

  for (const input of [ui.installDir, ui.repo, ui.javaPath, ui.javaArgs]) {
    input.addEventListener('change', () => void persist());
  }

  listen('install://progress', (payload) => {
    showProgress(payload.stage, payload.message, payload.percent);
    ui.subtitle.textContent = payload.message;
  });
  listen('dialog://folder', (value) => {
    if (typeof value === 'string' && value) { ui.installDir.value = value; void persist(); }
  });
  listen('dialog://java', (value) => {
    if (typeof value === 'string' && value) { ui.javaPath.value = value; void persist(); }
  });
}

async function poll() {
  try {
    const running = await invoke('is_game_running');
    if (running !== state.running) {
      state.running = running;
      setStatus(running ? 'Game is running' : 'Game closed', running ? 'ok' : '');
      setButtons();
    }
  } catch { /* transient while the game starts */ }
  setTimeout(poll, 1500);
}

async function start() {
  wire();
  try {
    await refresh();
    setStatus('Ready');
    if (ui.repo.value.trim()) void checkForUpdates();
  } catch (error) {
    setStatus(String(error), 'err');
  }
  void poll();
}

window.addEventListener('DOMContentLoaded', start);
