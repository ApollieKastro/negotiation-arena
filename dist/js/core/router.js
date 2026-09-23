// Рouter: history API + hash-совместимость (#/path), маршруты с :param

const routes = [];
let fallbackHandler = null;
let lastPath = null;
let started = false;

/** Регистрирует маршрут: route('/session/:id', (root, params) => ...) */
export function route(pattern, handler) {
  const keys = [];
  const source = String(pattern)
    .replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    .replace(/\\?:([A-Za-z0-9_]+)/g, (_, key) => {
      keys.push(key);
      return '([^/]+)';
    });
  routes.push({
    pattern,
    regex: new RegExp(`^${source}/?$`),
    keys,
    handler,
  });
}

/** Обработчик для неизвестных путей */
export function setFallback(handler) {
  fallbackHandler = handler;
}

/** Нормализует '#/x', 'x', '/x' → '/x' */
export function normalizePath(to) {
  let p = String(to || '/').trim();
  if (p.startsWith('#')) p = p.slice(1);
  if (!p.startsWith('/')) p = `/${p}`;
  const q = p.indexOf('?');
  if (q !== -1) p = p.slice(0, q);
  if (p.length > 1 && p.endsWith('/')) p = p.slice(0, -1);
  return p || '/';
}

/** Текущий путь: hash '#/...' приоритетнее pathname (прямой заход /login) */
export function currentPath() {
  const hash = location.hash;
  if (hash && hash.startsWith('#/')) {
    try { return normalizePath(decodeURIComponent(hash.slice(1))); }
    catch { return normalizePath(hash.slice(1)); }
  }
  return normalizePath(location.pathname || '/');
}

/** Переход: navigate('#/login') | navigate('/session/abc', {replace:true}) */
export function navigate(to, { replace = false } = {}) {
  const path = normalizePath(to);
  const url = `${location.pathname}${location.search}#${path}`;
  if (replace) history.replaceState(null, '', url);
  else history.pushState(null, '', url);
  resolve(true);
}

/** Корневой контейнер для хендлеров (root, params) */
let rootEl = null;
export function setRoot(el) {
  rootEl = el;
}

/** Запуск: навешивает popstate/hashchange и резолвит текущий путь */
export function start() {
  if (started) return;
  started = true;
  if (!rootEl) rootEl = document.getElementById('app');
  window.addEventListener('popstate', () => resolve(false));
  window.addEventListener('hashchange', () => resolve(false));
  // Прямой заход /login (без hash) читается из pathname — hash не подмешиваем
  resolve(true);
}

function resolve(force) {
  const path = currentPath();
  if (!force && path === lastPath) return;
  lastPath = path;

  for (const r of routes) {
    const m = path.match(r.regex);
    if (m) {
      const params = {};
      r.keys.forEach((k, i) => { params[k] = safeDecode(m[i + 1]); });
      r.handler(rootEl, params);
      return;
    }
  }
  if (fallbackHandler) fallbackHandler(path);
}

function safeDecode(v) {
  try { return decodeURIComponent(v); } catch { return v; }
}
