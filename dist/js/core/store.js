// Хранилище состояния: токен, юзер, тема/шрифт, синхронизация настроек

const TOKEN_KEY = 'na.token';
const USER_KEY = 'na.user';
const THEME_KEY = 'na.theme';
const FONT_KEY = 'na.fontSize';
const SETTINGS_KEY = 'na.settings';

const USER_SETTING_KEYS = ['theme', 'font_size', 'locale'];

/** JWT из sessionStorage */
export function accessToken() {
  try { return sessionStorage.getItem(TOKEN_KEY); } catch { return null; }
}

/** Текущий юзер из localStorage (или null) */
export function user() {
  try { return JSON.parse(localStorage.getItem(USER_KEY) || 'null'); } catch { return null; }
}

/** Сохраняет сессию после входа/регистрации; sync — best-effort PUT настроек */
export function setAuth(userData, session, { sync = true } = {}) {
  if (session && session.token) {
    try { sessionStorage.setItem(TOKEN_KEY, session.token); } catch { /* приватный режим */ }
  }
  if (userData) {
    try { localStorage.setItem(USER_KEY, JSON.stringify(userData)); } catch { /* ignore */ }
  }
  if (sync) pushLocalSettings().catch(() => {});
}

/** Полный выход: чистит токен и юзера (тему/шрифт оставляем) */
export function clear() {
  try { sessionStorage.removeItem(TOKEN_KEY); } catch { /* ignore */ }
  try { localStorage.removeItem(USER_KEY); } catch { /* ignore */ }
}

export function isLoggedIn() {
  return Boolean(accessToken() && user());
}

export function isAdmin() {
  const u = user();
  return Boolean(u && u.role === 'admin');
}

/** Обновляет только токен (после refresh), без повторной синхронизации */
export function updateSession(session) {
  if (session && session.token) {
    try { sessionStorage.setItem(TOKEN_KEY, session.token); } catch { /* ignore */ }
  }
  if (session && session.user) {
    try { localStorage.setItem(USER_KEY, JSON.stringify(session.user)); } catch { /* ignore */ }
  }
}

/**
 * Перезаписывает профиль в localStorage (смена логина/имени/аватара),
 * не трогая токен. После вызова разошлите `user-updated`, чтобы shell
 * (шапка/сайдбар) перечитал пользователя.
 */
export function setUser(userData) {
  if (!userData) return;
  try { localStorage.setItem(USER_KEY, JSON.stringify(userData)); } catch { /* ignore */ }
}

// ── Тема / шрифт ──

export function getTheme() {
  try { return localStorage.getItem(THEME_KEY) || 'dark'; } catch { return 'dark'; }
}

export function getFontSize() {
  try { return localStorage.getItem(FONT_KEY) || 'md'; } catch { return 'md'; }
}

/** Применяет тему к documentElement (поддержка system) */
export function applyTheme() {
  let theme = getTheme();
  if (theme === 'system') {
    theme = window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
  }
  document.documentElement.setAttribute('data-theme', theme);
}

/** Применяет размер шрифта */
export function applyFontSize() {
  document.documentElement.setAttribute('data-font', getFontSize());
}

/** Смена темы + best-effort синхронизация с сервером */
export function setTheme(theme) {
  try { localStorage.setItem(THEME_KEY, theme); } catch { /* ignore */ }
  applyTheme();
  if (isLoggedIn()) setSetting('theme', theme === 'system' ? 'system' : theme).catch(() => {});
}

/** Смена размера шрифта + best-effort синхронизация */
export function setFontSize(size) {
  try { localStorage.setItem(FONT_KEY, size); } catch { /* ignore */ }
  applyFontSize();
  if (isLoggedIn()) setSetting('font_size', size).catch(() => {});
}

/** Смена языка интерфейса + best-effort синхронизация с сервером */
export async function setLocale(locale) {
  const { setLocale: applyLocale } = await import('./i18n.js');
  applyLocale(locale);
  try { localStorage.setItem('na.locale', locale); } catch { /* ignore */ }
  cacheSettings({ locale });
  if (isLoggedIn()) {
    try { await setSetting('locale', locale); } catch { /* best-effort */ }
  }
  window.dispatchEvent(new CustomEvent('locale-changed', { detail: { locale } }));
}

/** Текущий язык (locale из настроек или ru) */
export function getLocale() {
  return getSettings().locale || 'ru';
}

/** Применяет локальные настройки (вызывается до/при старте) */
export function applyLocal() {
  applyTheme();
  applyFontSize();
}

// ── Настройки пользователя на сервере ──

/** Локальный кэш настроек (na.settings) */
export function getSettings() {
  try { return JSON.parse(localStorage.getItem(SETTINGS_KEY) || '{}'); } catch { return {}; }
}

function cacheSettings(map) {
  try { localStorage.setItem(SETTINGS_KEY, JSON.stringify({ ...getSettings(), ...map })); } catch { /* ignore */ }
}

/** PUT /settings/me/{key} — best-effort */
export async function setSetting(key, value) {
  const { request } = await import('./api.js');
  const res = await request(`/settings/me/${encodeURIComponent(key)}`, {
    method: 'PUT',
    body: { value: String(value) },
  });
  cacheSettings({ [key]: String(value) });
  return res;
}

/** PUSH локальных theme/font_size/locale на сервер (при входе) */
export async function pushLocalSettings() {
  if (!isLoggedIn()) return;
  const payload = {
    theme: getTheme(),
    font_size: getFontSize(),
    locale: getSettings().locale || 'ru',
  };
  await Promise.allSettled(
    USER_SETTING_KEYS.map((k) => setSetting(k, payload[k]))
  );
}

/** GET /settings/me → применяет к UI (сервер приоритетнее локальных) */
export async function pullServerSettings() {
  if (!isLoggedIn()) return null;
  const { request } = await import('./api.js');
  const map = await request('/settings/me');
  if (!map || typeof map !== 'object') return map;
  cacheSettings(map);
  if (map.theme) { try { localStorage.setItem(THEME_KEY, map.theme); } catch { /* ignore */ } applyTheme(); }
  if (map.font_size) { try { localStorage.setItem(FONT_KEY, map.font_size); } catch { /* ignore */ } applyFontSize(); }
  if (map.locale) {
    try { localStorage.setItem('na.locale', map.locale); } catch { /* ignore */ }
    const { setLocale: applyLocale, getLocale: currentLocale } = await import('./i18n.js');
    if (currentLocale() !== map.locale) {
      applyLocale(map.locale);
      window.dispatchEvent(new CustomEvent('locale-changed', { detail: { locale: map.locale } }));
    }
  }
  return map;
}
