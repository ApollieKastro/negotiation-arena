// История сессий: таблица, фильтр по статусу, переходы

import { h, fmtDateTime } from '../core/dom.js';
import { emptyState, skeleton, table, badge } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { navigate } from '../core/router.js';

const STATUS_LABEL = { active: 'Активна', finished: 'Завершена', abandoned: 'Брошена' };
const STATUS_VARIANT = { active: 'info', finished: 'success', abandoned: 'neutral' };
const FILTERS = [
  { id: '', label: 'Все' },
  { id: 'active', label: 'Активные' },
  { id: 'finished', label: 'Завершённые' },
  { id: 'abandoned', label: 'Брошенные' },
];

function statusBadge(status) {
  return badge(STATUS_LABEL[status] || status, STATUS_VARIANT[status] || 'neutral');
}

export function renderPage(root, params = {}) {
  let sessions = [];
  let titleMap = new Map();
  let statusFilter = '';
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'history' });
  const isCurrent = () => marker.isConnected;

  const host = h('div');

  const filterRow = h('div.tabs.tabs-pill.mt-3.mb-4', { role: 'tablist' });
  const filterBtns = FILTERS.map((f) => {
    const b = h('button.tab', {
      type: 'button',
      role: 'tab',
      class: `tab${f.id === '' ? ' is-active' : ''}`,
      'aria-selected': f.id === '' ? 'true' : 'false',
      text: f.label,
    });
    b.addEventListener('click', () => {
      statusFilter = f.id;
      filterBtns.forEach((x, i) => {
        const on = FILTERS[i].id === statusFilter;
        x.classList.toggle('is-active', on);
        x.setAttribute('aria-selected', on ? 'true' : 'false');
      });
      renderTable();
    });
    return b;
  });
  filterRow.append(...filterBtns);

  function visible() {
    return statusFilter
      ? sessions.filter((s) => s.status === statusFilter)
      : sessions;
  }

  function renderTable() {
    const rows = visible();
    if (!sessions.length) {
      host.replaceChildren(emptyState({
        icon: '☰',
        title: 'История пуста',
        description: 'Вы ещё не проходили ни одного сценария.',
        action: h('a.btn.btn-primary', { href: '#/scenarios' }, 'Выбрать сценарий'),
      }));
      return;
    }
    if (!rows.length) {
      host.replaceChildren(emptyState({
        icon: '∅',
        title: 'Нет сессий с таким статусом',
        description: 'Выберите другой фильтр.',
      }));
      return;
    }

    host.replaceChildren(table({
      columns: [
        { key: 'created_at', label: 'Дата', render: (r) => fmtDateTime(r.created_at) },
        {
          key: 'scenario_id',
          label: 'Сценарий',
          render: (r) => titleMap.get(r.scenario_id) || '—',
        },
        { key: 'status', label: 'Статус', render: (r) => statusBadge(r.status) },
        {
          key: 'total_score',
          label: 'Балл',
          align: 'right',
          mono: true,
          render: (r) => (r.status === 'finished' ? String(r.total_score) : '—'),
        },
        {
          key: 'turn_count',
          label: 'Ходов',
          align: 'right',
          mono: true,
          render: (r) => String(r.turn_count ?? 0),
        },
      ],
      rows,
      emptyText: 'Нет данных',
      onRowClick: (r) => {
        if (r.status === 'finished') navigate(`#/result/${r.id}`);
        else navigate(`#/session/${r.id}`);
      },
    }));
  }

  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: 'История' }),
        h('div.page-sub', null, 'Все ваши сессии — нажмите строку, чтобы открыть')
      )
    ),
    filterRow,
    host
  );

  host.append(skeleton(5, 36));

  Promise.all([
    request('/sessions', { query: { limit: 50 } }),
    request('/scenarios').catch(() => []),
  ]).then(([list, scenarios]) => {
    if (!isCurrent()) return;
    sessions = Array.isArray(list) ? list : [];
    titleMap = new Map((scenarios || []).map((s) => [s.id, s.title]));
    renderTable();
  }).catch((err) => {
    if (!isCurrent()) return;
    host.replaceChildren(emptyState({
      icon: '⚠',
      title: 'Не удалось загрузить историю',
      description: err instanceof ApiError ? err.message : 'Ошибка запроса',
      action: h('button.btn.btn-secondary', {
        type: 'button',
        text: 'Повторить',
        onClick: () => { if (isCurrent()) renderPage(root, params); },
      }),
    }));
  });
}
