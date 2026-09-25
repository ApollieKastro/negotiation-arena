// История сессий: таблица, фильтр по статусу, переходы

import { h, fmtDateTime } from '../core/dom.js';
import { emptyState, skeleton, table, badge } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { navigate } from '../core/router.js';
import { t, statusLabel } from '../core/i18n.js';

const STATUS_VARIANT = { active: 'info', finished: 'success', abandoned: 'neutral' };

function statusBadge(status) {
  return badge(statusLabel(status), STATUS_VARIANT[status] || 'neutral');
}

export function renderPage(root, params = {}) {
  let sessions = [];
  let total = 0;
  let offset = 0;
  const LIMIT = 50;
  let titleMap = new Map();
  let statusFilter = '';
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'history' });
  const isCurrent = () => marker.isConnected;

  const host = h('div');
  const pager = h('div.flex.items-center.justify-between.gap-2.mt-4');
  const prevBtn = h('button.btn.btn-secondary.btn-sm', { type: 'button', text: t('action.back') });
  const nextBtn = h('button.btn.btn-secondary.btn-sm', { type: 'button', text: t('action.next') });
  const pagerInfo = h('span.small.muted');
  prevBtn.addEventListener('click', () => {
    offset = Math.max(0, offset - LIMIT);
    loadPage();
  });
  nextBtn.addEventListener('click', () => {
    if (offset + LIMIT < total) {
      offset += LIMIT;
      loadPage();
    }
  });
  pager.append(prevBtn, pagerInfo, nextBtn);
  pager.style.display = 'none';

  const FILTERS = [
    { id: '', label: t('history.filter.all') },
    { id: 'active', label: t('history.filter.active') },
    { id: 'finished', label: t('history.filter.finished') },
    { id: 'abandoned', label: t('history.filter.abandoned') },
  ];

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
        title: t('history.empty'),
        description: t('history.emptyDesc'),
        action: h('a.btn.btn-primary', { href: '#/scenarios' }, t('action.pickScenario')),
      }));
      return;
    }
    if (!rows.length) {
      host.replaceChildren(emptyState({
        icon: '∅',
        title: t('history.noStatus'),
        description: t('history.noStatusDesc'),
      }));
      return;
    }

    host.replaceChildren(table({
      columns: [
        { key: 'created_at', label: t('history.col.date'), render: (r) => fmtDateTime(r.created_at) },
        {
          key: 'scenario_id',
          label: t('history.col.scenario'),
          render: (r) => titleMap.get(r.scenario_id) || '—',
        },
        { key: 'status', label: t('history.col.status'), render: (r) => statusBadge(r.status) },
        {
          key: 'total_score',
          label: t('history.col.score'),
          align: 'right',
          mono: true,
          render: (r) => (r.status === 'finished' ? String(r.total_score) : '—'),
        },
        {
          key: 'turn_count',
          label: t('history.col.turns'),
          align: 'right',
          mono: true,
          render: (r) => String(r.turn_count ?? 0),
        },
      ],
      rows,
      emptyText: t('common.noData'),
      onRowClick: (r) => {
        // Активная — продолжить игру; завершённая/брошенная — архив переписки
        if (r.status === 'active') navigate(`#/session/${r.id}`);
        else navigate(`#/dialog/${r.id}`);
      },
    }));
  }

  function updatePager() {
    const hasMore = offset + LIMIT < total;
    pager.style.display = total > 0 ? 'flex' : 'none';
    prevBtn.disabled = offset === 0;
    nextBtn.disabled = !hasMore;
    if (total === 0) {
      pagerInfo.textContent = '';
      return;
    }
    const from = offset + 1;
    const to = Math.min(offset + sessions.length, total);
    pagerInfo.textContent = t('history.pager', { from, to, total });
  }

  function loadPage() {
    if (!isCurrent()) return;
    host.replaceChildren(skeleton(5, 36));
    updatePager();
    Promise.all([
      request('/sessions', { query: { limit: LIMIT, offset } }),
      request('/scenarios').catch(() => []),
    ]).then(([page, scenarios]) => {
      if (!isCurrent()) return;
      if (Array.isArray(page)) {
        sessions = page;
        total = page.length;
        offset = 0;
      } else {
        sessions = Array.isArray(page?.items) ? page.items : [];
        total = Number(page?.total ?? sessions.length) || 0;
      }
      titleMap = new Map((scenarios || []).map((s) => [s.id, s.title]));
      updatePager();
      renderTable();
    }).catch((err) => {
      if (!isCurrent()) return;
      host.replaceChildren(emptyState({
        icon: '⚠',
        title: t('history.loadError'),
        description: err instanceof ApiError ? err.message : t('common.error'),
        action: h('button.btn.btn-secondary', {
          type: 'button',
          text: t('action.retry'),
          onClick: () => { if (isCurrent()) loadPage(); },
        }),
      }));
    });
  }

  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: t('history.title') }),
        h('div.page-sub', null, t('history.sub'))
      )
    ),
    filterRow,
    host,
    pager
  );

  loadPage();
}
