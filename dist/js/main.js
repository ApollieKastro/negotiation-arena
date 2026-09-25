// Точка входа: таблица маршрутов, shell (sidebar + topbar), guard'ы

import { h, clear } from './core/dom.js';
import * as store from './core/store.js';
import { t, initLocale, getLocale } from './core/i18n.js';
import { navigate, route, start, setFallback, setRoot, currentPath } from './core/router.js';
import { avatar } from './core/components.js';

import * as loginPage from './pages/login.js';
import * as homePage from './pages/home.js';
import * as scenariosPage from './pages/scenarios.js';
import * as sessionPage from './pages/session.js';
import * as callPage from './pages/call.js';
import * as dialogPage from './pages/dialog.js';
import * as resultPage from './pages/result.js';
import * as settingsPage from './pages/settings.js';
import * as profilePage from './pages/profile.js';
import * as historyPage from './pages/history.js';
import * as leaderboardPage from './pages/leaderboard.js';
import * as teamPage from './pages/team.js';

import * as adminDashboard from './pages/admin/dashboard.js';
import * as adminUsers from './pages/admin/users.js';
import * as adminScenarios from './pages/admin/scenarios.js';
import * as adminProviders from './pages/admin/providers.js';
import * as adminSettings from './pages/admin/settings.js';
import * as adminAudit from './pages/admin/audit.js';

const appRoot = document.getElementById('app');

// ── Таблица маршрутов ──
// access: 'public' | 'user' | 'admin'; admin недоступен не-админу (→ #/)
// title — ключ i18n (см. core/i18n.js)
const ROUTES = [
  { pattern: '/login', page: loginPage, access: 'public', titleKey: 'login.title' },
  { pattern: '/register', page: loginPage, access: 'public', titleKey: 'login.registerTitle' },

  { pattern: '/', page: homePage, access: 'user', titleKey: 'nav.home' },
  { pattern: '/scenarios', page: scenariosPage, access: 'user', titleKey: 'scenarios.title' },
  { pattern: '/session/:id', page: sessionPage, access: 'user', titleKey: 'session.title' },
  { pattern: '/call/:id', page: callPage, access: 'user', titleKey: 'call.title' },
  { pattern: '/dialog/:id', page: dialogPage, access: 'user', titleKey: 'dialog.title' },
  { pattern: '/result/:id', page: resultPage, access: 'user', titleKey: 'result.title' },
  { pattern: '/settings', page: settingsPage, access: 'user', titleKey: 'settings.title' },
  { pattern: '/profile', page: profilePage, access: 'user', titleKey: 'profile.title' },
  { pattern: '/settings/team', page: teamPage, access: 'user', titleKey: 'team.title' },
  { pattern: '/history', page: historyPage, access: 'user', titleKey: 'history.title' },
  { pattern: '/leaderboard', page: leaderboardPage, access: 'user', titleKey: 'leaderboard.title' },

  { pattern: '/admin', page: adminDashboard, access: 'admin', titleKey: 'nav.dashboard' },
  { pattern: '/admin/users', page: adminUsers, access: 'admin', titleKey: 'nav.users' },
  { pattern: '/admin/scenarios', page: adminScenarios, access: 'admin', titleKey: 'nav.scenarios' },
  { pattern: '/admin/providers', page: adminProviders, access: 'admin', titleKey: 'nav.providers' },
  { pattern: '/admin/settings', page: adminSettings, access: 'admin', titleKey: 'nav.settings' },
  { pattern: '/admin/audit', page: adminAudit, access: 'admin', titleKey: 'nav.audit' },
];

// ── Shell ──
let shell = null;

function navPlay() {
  return [
    { href: '#/', label: t('nav.home'), icon: '⌂', match: '/' },
    { href: '#/scenarios', label: t('nav.scenarios'), icon: '◈', match: '/scenarios' },
    { href: '#/history', label: t('nav.history'), icon: '☰', match: '/history' },
    { href: '#/leaderboard', label: t('nav.leaderboard'), icon: '★', match: '/leaderboard' },
  ];
}

function navAdmin() {
  return [
    { href: '#/admin', label: t('nav.dashboard'), icon: '▦', match: '/admin' },
    { href: '#/admin/users', label: t('nav.users'), icon: '☺', match: '/admin/users' },
    { href: '#/admin/scenarios', label: t('nav.scenarios'), icon: '◈', match: '/admin/scenarios' },
    { href: '#/admin/providers', label: t('nav.providers'), icon: '⚙', match: '/admin/providers' },
    { href: '#/admin/settings', label: t('nav.settings'), icon: '☰', match: '/admin/settings' },
    { href: '#/admin/audit', label: t('nav.audit'), icon: '≡', match: '/admin/audit' },
  ];
}

function buildNavGroup(title, items) {
  const group = h('div.nav-group', null, h('div.nav-group-title', { text: title }));
  for (const item of items) {
    const a = h('a.nav-item', { href: item.href, dataset: { match: item.match } },
      h('span.nav-icon', { text: item.icon }),
      h('span', { text: item.label })
    );
    group.append(a);
  }
  return group;
}

function toggleTheme() {
  const current = document.documentElement.getAttribute('data-theme');
  const next = current === 'light' ? 'dark' : 'light';
  store.setTheme(next);
  if (shell) shell.themeBtn.textContent = next === 'light' ? '☾' : '☀';
}

function logout() {
  store.clear();
  closeSidebar();
  navigate('#/login', { replace: true });
}

function ensureShell() {
  if (shell) return shell;

  const u = store.user() || {};
  const themeNow = document.documentElement.getAttribute('data-theme');

  const sidebar = h('aside.sidebar', { id: 'sidebar' },
    h('div.sidebar-brand', null,
      h('span.brand-mark', null,
        h('img.brand-mark-img', {
          src: '/static/team-logo-mark.png',
          alt: '',
          decoding: 'async',
          'aria-hidden': 'true',
        })
      ),
      h('span', { text: t('app.brand') })
    ),
    h('nav.sidebar-nav', null,
      buildNavGroup(t('nav.play'), navPlay()),
      store.isAdmin() ? buildNavGroup(t('nav.admin'), navAdmin()) : null
    ),
    h('div.sidebar-footer', null,
      h('a.user-card', { href: '#/profile', title: t('profile.title') },
        avatar(u),
        h('div.user-card-meta', null,
          h('div.user-card-name', { text: u.display_name || u.login || '—' }),
          h('div.user-card-role', { text: u.role === 'admin' ? t('role.admin') : t('role.user') })
        )
      ),
      h('button.btn.btn-ghost.btn-block', { type: 'button', onClick: logout }, t('action.logout'))
    )
  );

  const overlay = h('div.sidebar-overlay', { onClick: closeSidebar });

  const burger = h('button.icon-btn.burger', {
    type: 'button', 'aria-label': 'Меню', text: '☰',
    onClick: () => {
      sidebar.classList.toggle('is-open');
      overlay.classList.toggle('is-open');
    },
  });

  const titleEl = h('h1.topbar-title', { text: '—' });
  const themeBtn = h('button.icon-btn', {
    type: 'button', 'aria-label': 'Переключить тему',
    text: themeNow === 'light' ? '☾' : '☀',
    onClick: toggleTheme,
  });

  // Юзер-меню в topbar
  const menuList = h('div.user-menu-list', null,
    h('div.user-menu-head', null,
      h('strong', { text: u.display_name || u.login || '—' }),
      h('span', { text: u.role === 'admin' ? t('role.admin') : t('role.user') })
    ),
    h('a.menu-item', { href: '#/profile' }, t('menu.profile')),
    h('a.menu-item', { href: '#/settings' }, t('menu.settings')),
    h('a.menu-item', { href: '#/settings/team' }, t('menu.team')),
    h('button.menu-item.danger', { type: 'button', onClick: logout }, t('menu.logout'))
  );
  const userMenu = h('div.user-menu', null,
    h('button.user-menu-btn', {
      type: 'button',
      'aria-label': 'Меню пользователя',
      onClick: (e) => {
        e.stopPropagation();
        userMenu.classList.toggle('is-open');
      },
    },
      avatar(u, 'sm'),
      h('span.user-menu-label', { text: u.login || '' }),
      h('span', { text: '▾' })
    ),
    menuList
  );
  document.addEventListener('click', () => userMenu.classList.remove('is-open'));

  const topbar = h('header.topbar', null,
    burger,
    titleEl,
    h('div.topbar-actions', null, themeBtn, userMenu)
  );

  const main = h('main.main-content', { id: 'page' });
  const mainCol = h('div.main-col', null, topbar, main);
  const shellEl = h('div.app-shell', null, sidebar, overlay, mainCol);

  clear(appRoot);
  appRoot.append(shellEl);

  shell = {
    el: shellEl,
    sidebar,
    overlay,
    main,
    titleEl,
    themeBtn,
    setTitle(text) { titleEl.textContent = text; },
  };
  return shell;
}

function closeSidebar() {
  if (!shell) return;
  shell.sidebar.classList.remove('is-open');
  shell.overlay.classList.remove('is-open');
}

function destroyShell() {
  shell = null;
}

function updateActiveNav(path) {
  if (!shell) return;
  const norm = path.length > 1 && path.endsWith('/') ? path.slice(0, -1) : path;
  shell.sidebar.querySelectorAll('.nav-item').forEach((a) => {
    a.classList.toggle('is-active', a.dataset.match === norm);
  });
}

// Публичная страница без shell (login/register)
function renderBare(page, params) {
  destroyShell();
  const host = h('div.auth-layout');
  clear(appRoot);
  appRoot.append(host);
  page.renderPage(host, params);
}

// ── Регистрация маршрутов: хендлер (root, params) ──
for (const r of ROUTES) {
  route(r.pattern, (root, params) => {
    // Guard: без токена → #/login (кроме login/register)
    if (r.access !== 'public' && !store.isLoggedIn()) {
      navigate('#/login', { replace: true });
      return;
    }
    // Уже вошёл и открыл login/register → в кабинет
    if (r.access === 'public' && store.isLoggedIn()) {
      navigate('#/', { replace: true });
      return;
    }
    // Админ-маршрут не для role=user → #/
    if (r.access === 'admin' && !store.isAdmin()) {
      navigate('#/', { replace: true });
      return;
    }

    if (r.access === 'public') {
      const mode = r.pattern === '/register' ? 'register' : 'login';
      renderBare(r.page, { ...params, mode });
      const title = t(r.titleKey);
      document.title = `${title} — ${t('app.brand')}`;
      return;
    }

    const s = ensureShell();
    const title = t(r.titleKey);
    s.setTitle(title);
    document.title = `${title} — ${t('app.brand')}`;
    clear(s.main);
    r.page.renderPage(s.main, params);
    updateActiveNav(currentPath());
    closeSidebar();
  });
}

// Неизвестный маршрут
setFallback(() => {
  navigate(store.isLoggedIn() ? '#/' : '#/login', { replace: true });
});

// ── Смена языка: пересобрать shell и текущую страницу ──
window.addEventListener('locale-changed', () => {
  destroyShell();
  const path = currentPath();
  const mode = path === '/register' ? 'register' : 'login';
  const r = ROUTES.find((x) => {
    const src = x.pattern.replace(/:\w+/g, '[^/]+');
    return new RegExp(`^${src}/?$`).test(path);
  });

  if (r && r.access === 'public') {
    // bare login/register — без shell
    destroyShell();
    const host = h('div.auth-layout');
    clear(appRoot);
    appRoot.append(host);
    r.page.renderPage(host, { mode });
    const title = t(r.titleKey);
    document.title = `${title} — ${t('app.brand')}`;
    return;
  }

  navigate(`#${path}`, { replace: true });
  if (r) {
    const s = ensureShell();
    const title = t(r.titleKey);
    s.setTitle(title);
    document.title = `${title} — ${t('app.brand')}`;
    clear(s.main);
    r.page.renderPage(s.main, {});
    updateActiveNav(path);
  }
});

// ── Обновление профиля (логин/имя/аватар): shell перечитывает store.user ──
window.addEventListener('user-updated', () => {
  destroyShell();
  navigate(`#${currentPath()}`, { replace: true });
});

// ── Старт ──
initLocale();
store.applyLocal(); // локальные theme/font сразу
setRoot(appRoot);
start();

// Затем подтягиваем настройки с сервера (если вошли)
if (store.isLoggedIn()) {
  store.pullServerSettings().catch(() => {});
}
