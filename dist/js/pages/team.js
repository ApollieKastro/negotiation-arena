// Страница «О команде»: логотип, участники, описание.
// Список участников заполняется здесь — поля структурированы под будущие данные.

import { h } from '../core/dom.js';
import { t } from '../core/i18n.js';

/**
 * Участники команды.
 * Заполните, когда будут данные: name, role, bio, meta (например, «Backend»),
 * links: [{ label, href }] — необязательно.
 * @type {Array<{name: string, role: string, bio?: string, meta?: string, links?: Array<{label: string, href: string}>}>}
 */
const TEAM_MEMBERS = [
  // Пример (удалить/заменить):
  // {
  //   name: 'Иван Иванов',
  //   role: 'Team lead',
  //   bio: 'Архитектура, backend, интеграции.',
  //   meta: 'Go / Rust',
  //   links: [{ label: 'GitHub', href: 'https://github.com/…' }],
  // },
];

function initials(name) {
  return String(name || '?')
    .trim()
    .split(/\s+/)
    .slice(0, 2)
    .map((p) => p[0] || '')
    .join('')
    .toUpperCase() || '?';
}

function memberCard(m) {
  const links = Array.isArray(m.links) ? m.links.filter((l) => l && l.href) : [];
  return h('article.team-member', null,
    h('span.team-member-avatar', { text: initials(m.name), 'aria-hidden': 'true' }),
    h('div.team-member-name', { text: m.name || '—' }),
    m.role ? h('div.team-member-role', { text: m.role }) : null,
    m.bio ? h('p.team-member-bio', { text: m.bio }) : null,
    m.meta ? h('div.team-member-meta', { text: m.meta }) : null,
    links.length
      ? h('div.team-links', null,
        ...links.map((l) => h('a', {
          href: l.href,
          target: '_blank',
          rel: 'noopener noreferrer',
          text: l.label || l.href,
        }))
      )
      : null
  );
}

/** Контракт страницы: рендер в root (DOM-контейнер) + params роутера */
export function renderPage(root, params = {}) {
  void params;
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'team' });

  const grid = h('div.team-grid');
  if (!TEAM_MEMBERS.length) {
    grid.append(h('div.team-empty', { text: t('team.empty') }));
  } else {
    for (const m of TEAM_MEMBERS) grid.append(memberCard(m));
  }

  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: t('team.title') }),
        h('div.page-sub', null, t('team.sub'))
      )
    ),

    h('div.card.mt-4', null,
      h('div.team-hero', null,
        h('img.brand-logo', {
          src: '/static/team-logo.png',
          alt: t('app.team'),
          width: 420,
          height: 140,
          decoding: 'async',
        }),
        h('h2', { text: t('app.team') }),
        h('p.team-tagline', { text: t('team.tagline') })
      )
    ),

    h('div.card.mt-4', null,
      h('div.card-body.stack', { style: { gap: 'var(--sp-4)' } },
        h('div.stack', { style: { gap: '2px' } },
          h('div.card-title', { text: t('team.members') }),
          h('div.small.muted', { text: t('team.membersDesc') })
        ),
        grid
      )
    )
  );
}
