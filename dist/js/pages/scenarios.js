// Каталог сценариев: фильтры, карточки, старт, модалка «Подробнее»

import { h } from '../core/dom.js';
import {
  emptyState, skeleton, toast, badge, modal,
} from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { navigate } from '../core/router.js';
import { t, difficultyLabel } from '../core/i18n.js';

const DIFF_VARIANT = { easy: 'success', medium: 'warning', hard: 'danger' };

function truncate(text, max = 150) {
  const s = String(text || '');
  return s.length > max ? `${s.slice(0, max).trimEnd()}…` : s;
}

async function startSession(scenarioId, btn, { mode = null, to = 'session' } = {}) {
  if (btn) { btn.disabled = true; btn.textContent = t('action.starting'); }
  try {
    const body = { scenario_id: scenarioId };
    if (mode) body.mode = mode;
    const started = await request('/sessions', { method: 'POST', body });
    toast(t('scenarios.started'), 'success');
    navigate(to === 'call' ? `#/call/${started.session.id}` : `#/session/${started.session.id}`);
  } catch (err) {
    toast(err instanceof ApiError ? err.message : t('scenarios.startFail'), 'error');
    if (btn) { btn.disabled = false; btn.textContent = t('action.start'); }
  }
}

function detailRow(label, value, hint) {
  if (value === null || value === undefined || value === '' ||
      (Array.isArray(value) && !value.length)) return null;
  return h('div.stack', { style: { gap: '2px' } },
    h('div.small.muted', { text: label, title: hint || undefined }),
    hint ? h('div.small.muted', { style: { fontStyle: 'italic' }, text: hint }) : null,
    Array.isArray(value)
      ? h('div', null, ...value.map((v, i) =>
          h('div', { text: (value.length > 1 ? `${i + 1}. ` : '') + String(v) })))
      : h('div', { text: String(value) })
  );
}

function openDetails(s, onStart) {
  const p = s.partner_personality || {};
  const personalityBits = [
    p.tone && `тон: ${p.tone}`,
    p.style && `манера: ${p.style}`,
    p.traits && `черты: ${p.traits}`,
  ].filter(Boolean).join(', ');

  const body = h('div.stack', { style: { gap: 'var(--sp-4)' } },
    h('div.row', null,
      badge(difficultyLabel(s.difficulty) || s.difficulty, DIFF_VARIANT[s.difficulty] || 'neutral'),
      badge(s.sphere || '—', 'info'),
      s.is_active ? badge(t('scenarios.active'), 'success') : badge(t('scenarios.draft'), 'neutral')
    ),
    h('p', { text: s.description || '', style: { color: 'var(--muted)' } }),

    h('div.grid-2', null,
      h('div.card', null, h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
        h('div.card-title', { text: t('scenarios.you') }),
        detailRow(t('scenarios.role'), s.player_role),
        detailRow(t('scenarios.company'), s.player_company),
        detailRow(t('scenarios.goal'), s.player_goal),
        detailRow(t('scenarios.batna'), s.player_batna, t('scenarios.batnaHint'))
      )),
      h('div.card', null, h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
        h('div.card-title', { text: t('scenarios.partner', { name: s.partner_name }) }),
        detailRow(t('scenarios.role'), s.partner_role),
        detailRow(t('scenarios.company'), s.partner_company),
        detailRow(t('scenarios.goal'), s.partner_goal),
        detailRow(t('scenarios.extraGoals'), s.partner_goals),
        detailRow(t('scenarios.batna'), s.partner_batna, t('scenarios.batnaHint')),
        detailRow(t('scenarios.personality'), personalityBits || '—')
      ))
    ),

    s.endings && s.endings.length
      ? h('div.stack', { style: { gap: 'var(--sp-2)' } },
          h('div.small.muted', { text: t('scenarios.endings') }),
          h('div.row', null, ...s.endings.map((e) =>
            h('span.badge.badge-accent.no-dot', {
              text: t('scenarios.fromScore', { title: e.title, score: e.min_score }),
            })))
        )
      : null
  );

  modal({
    title: s.title,
    body,
    actions: [
      { label: t('action.close'), variant: 'secondary' },
      { label: t('action.start'), variant: 'primary', onClick: () => { onStart(); } },
    ],
  });
}

function scenarioCard(s) {
  const startBtn = h('button.btn.btn-primary.btn-sm', {
    type: 'button',
    text: t('action.start'),
    onClick: () => startSession(s.id, startBtn),
  });
  const callBtn = h('button.btn.btn-secondary.btn-sm', {
    type: 'button',
    text: t('call.button'),
    onClick: () => startSession(s.id, callBtn, { mode: 'voice', to: 'call' }),
  });
  const moreBtn = h('button.btn.btn-ghost.btn-sm', {
    type: 'button',
    text: t('action.more'),
    onClick: () => openDetails(s, () => startSession(s.id, startBtn)),
  });

  return h('div.card', null,
    h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
      h('div.row-between', null,
        h('div.card-title', { text: s.title }),
        badge(difficultyLabel(s.difficulty) || s.difficulty, DIFF_VARIANT[s.difficulty] || 'neutral')
      ),
      h('div.small.muted', { text: s.sphere || '' }),
      h('div', { text: truncate(s.description, 150), style: { color: 'var(--muted)' } }),
      s.player_goal
        ? h('div.small', null,
            h('span.muted', { text: t('home.goal') }),
            h('span', { text: truncate(s.player_goal, 110) }))
        : null,
      h('div.row', null, startBtn, callBtn, moreBtn)
    )
  );
}

export function renderPage(root, params = {}) {
  let all = [];
  let difficulty = '';
  let query = '';
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'scenarios' });
  const isCurrent = () => marker.isConnected;

  const grid = h('div.grid-2');
  const filterBtns = [];

  const searchInput = h('input.input', {
    type: 'search',
    placeholder: t('scenarios.search'),
    'aria-label': t('scenarios.search'),
  });
  searchInput.addEventListener('input', () => {
    query = searchInput.value.trim().toLowerCase();
    renderGrid();
  });

  function setDifficulty(id) {
    difficulty = id;
    filterBtns.forEach((b) => b.classList.toggle('is-active', b.dataset.id === id));
    renderGrid();
  }

  const diffs = [
    { id: '', label: t('diff.all') },
    { id: 'easy', label: t('diff.easy') },
    { id: 'medium', label: t('diff.medium') },
    { id: 'hard', label: t('diff.hard') },
  ];
  const filterRow = h('div.row', null, ...diffs.map((d) => {
    const b = h('button.tab', {
      type: 'button',
      class: `tab${d.id === '' ? ' is-active' : ''}`,
      text: d.label,
    });
    b.dataset.id = d.id;
    b.addEventListener('click', () => setDifficulty(d.id));
    filterBtns.push(b);
    return b;
  }));

  function visible() {
    return all.filter((s) => {
      if (s.is_active === false) return false;
      if (difficulty && s.difficulty !== difficulty) return false;
      if (query && !String(s.title || '').toLowerCase().includes(query)) return false;
      return true;
    });
  }

  function renderGrid() {
    const list = visible();
    if (!list.length) {
      grid.replaceChildren(emptyState({
        icon: '∅',
        title: t('scenarios.notFound'),
        description: all.length
          ? t('scenarios.notFoundFilter')
          : t('scenarios.notPublished'),
      }));
      return;
    }
    grid.replaceChildren(...list.map(scenarioCard));
  }

  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: t('scenarios.title') }),
        h('div.page-sub', null, t('scenarios.sub'))
      )
    ),
    h('div.row.mt-3.mb-4', { style: { justifyContent: 'space-between' } },
      filterRow,
      h('div', { style: { minWidth: '240px', flex: '1', maxWidth: '320px' } }, searchInput)
    ),
    grid
  );

  grid.append(skeleton(3, 140));
  request('/scenarios').then((list) => {
    if (!isCurrent()) return;
    all = Array.isArray(list) ? list : [];
    renderGrid();
  }).catch((err) => {
    if (!isCurrent()) return;
    grid.replaceChildren(emptyState({
      icon: '⚠',
      title: t('scenarios.loadError'),
      description: err instanceof ApiError ? err.message : t('common.error'),
      action: h('button.btn.btn-secondary', {
        type: 'button',
        text: t('action.retry'),
        onClick: () => { if (isCurrent()) renderPage(root, params); },
      }),
    }));
  });
}
