// Каталог сценариев: фильтры, карточки, старт, модалка «Подробнее»

import { h } from '../core/dom.js';
import {
  emptyState, skeleton, toast, badge, modal,
} from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { navigate } from '../core/router.js';

const DIFFS = [
  { id: '', label: 'Все' },
  { id: 'easy', label: 'Начальная' },
  { id: 'medium', label: 'Средняя' },
  { id: 'hard', label: 'Сложная' },
];
const DIFF_LABEL = { easy: 'Начальная', medium: 'Средняя', hard: 'Сложная' };
const DIFF_VARIANT = { easy: 'success', medium: 'warning', hard: 'danger' };

function truncate(text, max = 150) {
  const s = String(text || '');
  return s.length > max ? `${s.slice(0, max).trimEnd()}…` : s;
}

async function startSession(scenarioId, btn) {
  if (btn) { btn.disabled = true; btn.textContent = 'Старт…'; }
  try {
    const started = await request('/sessions', {
      method: 'POST',
      body: { scenario_id: scenarioId },
    });
    toast('Сессия начата — удачи!', 'success');
    navigate(`#/session/${started.session.id}`);
  } catch (err) {
    toast(err instanceof ApiError ? err.message : 'Не удалось начать сессию', 'error');
    if (btn) { btn.disabled = false; btn.textContent = 'Начать'; }
  }
}

function detailRow(label, value) {
  if (value === null || value === undefined || value === '' ||
      (Array.isArray(value) && !value.length)) return null;
  return h('div.stack', { style: { gap: '2px' } },
    h('div.small.muted', { text: label }),
    Array.isArray(value)
      ? h('div', null, ...value.map((v, i) =>
          h('div', { text: (value.length > 1 ? `${i + 1}. ` : '') + String(v) })))
      : h('div', { text: String(value) })
  );
}

function openDetails(s, onStart) {
  const p = s.partner_personality || {};
  const personalityBits = [p.tone && `тон: ${p.tone}`, p.style && `манера: ${p.style}`, p.traits && `черты: ${p.traits}`]
    .filter(Boolean).join(', ');

  const body = h('div.stack', { style: { gap: 'var(--sp-4)' } },
    h('div.row', null,
      badge(DIFF_LABEL[s.difficulty] || s.difficulty, DIFF_VARIANT[s.difficulty] || 'neutral'),
      badge(s.sphere || '—', 'info'),
      s.is_active ? badge('Активный', 'success') : badge('Черновик', 'neutral')
    ),
    h('p', { text: s.description || '', style: { color: 'var(--muted)' } }),

    h('div.grid-2', null,
      h('div.card', null, h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
        h('div.card-title', { text: 'Ты' }),
        detailRow('Роль', s.player_role),
        detailRow('Компания', s.player_company),
        detailRow('Цель', s.player_goal),
        detailRow('BATNA', s.player_batna)
      )),
      h('div.card', null, h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
        h('div.card-title', { text: `Собеседник: ${s.partner_name}` }),
        detailRow('Роль', s.partner_role),
        detailRow('Компания', s.partner_company),
        detailRow('Цель', s.partner_goal),
        detailRow('Доп. цели', s.partner_goals),
        detailRow('BATNA', s.partner_batna),
        detailRow('Личность', personalityBits || '—')
      ))
    ),

    s.endings && s.endings.length
      ? h('div.stack', { style: { gap: 'var(--sp-2)' } },
          h('div.small.muted', { text: 'Возможные финалы' }),
          h('div.row', null, ...s.endings.map((e) =>
            h('span.badge.badge-accent.no-dot', { text: `${e.title} (от ${e.min_score})` })))
        )
      : null
  );

  modal({
    title: s.title,
    body,
    actions: [
      { label: 'Закрыть', variant: 'secondary' },
      { label: 'Начать', variant: 'primary', onClick: () => { onStart(); } },
    ],
  });
}

function scenarioCard(s) {
  const startBtn = h('button.btn.btn-primary.btn-sm', {
    type: 'button',
    text: 'Начать',
    onClick: () => startSession(s.id, startBtn),
  });
  const moreBtn = h('button.btn.btn-ghost.btn-sm', {
    type: 'button',
    text: 'Подробнее',
    onClick: () => openDetails(s, () => startSession(s.id, startBtn)),
  });

  return h('div.card', null,
    h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
      h('div.row-between', null,
        h('div.card-title', { text: s.title }),
        badge(DIFF_LABEL[s.difficulty] || s.difficulty, DIFF_VARIANT[s.difficulty] || 'neutral')
      ),
      h('div.small.muted', { text: s.sphere || '' }),
      h('div', { text: truncate(s.description, 150), style: { color: 'var(--muted)' } }),
      s.player_goal
        ? h('div.small', null,
            h('span.muted', { text: 'Цель: ' }),
            h('span', { text: truncate(s.player_goal, 110) }))
        : null,
      h('div.row', null, startBtn, moreBtn)
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
    placeholder: 'Поиск по названию…',
    'aria-label': 'Поиск по названию',
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

  const filterRow = h('div.row', null, ...DIFFS.map((d) => {
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
        title: 'Ничего не найдено',
        description: all.length
          ? 'Попробуйте изменить фильтр или поисковый запрос.'
          : 'Сценарии ещё не опубликованы администратором.',
      }));
      return;
    }
    grid.replaceChildren(...list.map(scenarioCard));
  }

  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: 'Сценарии' }),
        h('div.page-sub', null, 'Выберите переговоры и начните диалог с ИИ')
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
      title: 'Не удалось загрузить сценарии',
      description: err instanceof ApiError ? err.message : 'Ошибка запроса',
      action: h('button.btn.btn-secondary', {
        type: 'button',
        text: 'Повторить',
        onClick: () => { if (isCurrent()) renderPage(root, params); },
      }),
    }));
  });
}
