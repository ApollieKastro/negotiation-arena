// Аудит-лог: фильтры (action, user_id, limit), таблица, пустое состояние

import { h, fmtDateTime } from '../../core/dom.js';
import { request, ApiError } from '../../core/api.js';
import { table, field, toast, skeleton, emptyState } from '../../core/components.js';

function errMsg(err, fallback) {
  return err instanceof ApiError ? err.message : fallback;
}

const DETAILS_LIMIT = 100;

function detailsCell(value) {
  if (!value) return null;
  const s = String(value);
  if (s.length <= DETAILS_LIMIT) return h('span', { text: s, title: s });
  return h('span', { text: `${s.slice(0, DETAILS_LIMIT)}…`, title: s });
}

/** Контракт страницы: рендер в root (DOM-контейнер) + params роутера */
export function renderPage(root, params = {}) {
  let rows = [];
  let loaded = false;

  const actionF = field({
    label: 'Action',
    placeholder: 'auth.login',
    hint: 'Точное совпадение, напр. auth.login, user.role, session.start',
  });
  const userF = field({
    label: 'Исполнитель (user_id)',
    placeholder: 'UUID пользователя',
    hint: 'Пусто — все пользователи',
  });
  const limitF = field({
    label: 'Лимит', type: 'select', value: '50',
    options: [
      { value: '50', label: '50 записей' },
      { value: '100', label: '100 записей' },
      { value: '200', label: '200 записей' },
    ],
  });

  const tableHost = h('div');
  const countHost = h('span.small.muted');

  const applyBtn = h('button.btn.btn-primary', { type: 'submit', text: 'Применить' });
  const resetBtn = h('button.btn.btn-secondary', { type: 'button', text: 'Сбросить' });
  resetBtn.addEventListener('click', () => {
    actionF.control.value = '';
    userF.control.value = '';
    limitF.control.value = '50';
    load();
  });

  const form = h('form', { novalidate: true },
    h('div.row', null, actionF, userF, limitF),
    h('div.row.mt-3', null, applyBtn, resetBtn, countHost)
  );
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    load();
  });
  limitF.control.addEventListener('change', () => load());

  root.replaceChildren(
    h('div.page-header', null,
      h('div', null,
        h('h1', null, 'Аудит'),
        h('div.page-sub', null, 'Журнал действий: входы, роли, сценарии, настройки')
      )
    ),
    h('div.card', null,
      h('div.card-header', null,
        h('div.card-title', { text: 'Фильтры' }),
        h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Обновить', onClick: () => load() })
      ),
      h('div.card-body', null, form)
    ),
    h('div.card.mt-4', null,
      h('div.card-header', null, h('div.card-title', { text: 'Записи' })),
      tableHost
    )
  );

  tableHost.append(skeleton(8));

  async function load() {
    if (!loaded) tableHost.replaceChildren(skeleton(8));
    const query = { limit: limitF.control.value };
    const action = actionF.control.value.trim();
    const userId = userF.control.value.trim();
    if (action) query.action = action;
    if (userId) query.user_id = userId;

    try {
      rows = await request('/audit', { query });
      loaded = true;
      renderTable();
    } catch (err) {
      loaded = true;
      tableHost.replaceChildren(
        h('div.card-body', null,
          h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить журнал') })
        )
      );
      toast(errMsg(err, 'Не удалось загрузить журнал'), 'error');
    }
  }

  function renderTable() {
    countHost.textContent = `${rows.length} записей`;

    if (!rows.length) {
      tableHost.replaceChildren(
        emptyState({
          icon: '≡',
          title: 'Записей нет',
          description: 'По текущим фильтрам ничего не найдено. Попробуйте сбросить фильтр или изменить action.',
        })
      );
      return;
    }

    tableHost.replaceChildren(
      table({
        columns: [
          { key: 'created_at', label: 'Когда', render: (r) => fmtDateTime(r.created_at) },
          {
            key: 'user_id',
            label: 'Исполнитель',
            render: (r) => (r.user_id
              ? h('span', { text: r.user_id, title: r.user_id, style: { fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' } })
              : null),
          },
          {
            key: 'action',
            label: 'Action',
            render: (r) => h('span', { text: r.action, style: { fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' } }),
          },
          { key: 'entity', label: 'Сущность' },
          {
            key: 'entity_id',
            label: 'ID',
            render: (r) => (r.entity_id
              ? h('span', {
                  text: r.entity_id,
                  title: r.entity_id,
                  style: { fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' },
                })
              : null),
          },
          { key: 'details', label: 'Детали', render: (r) => detailsCell(r.details) },
        ],
        rows,
        emptyText: 'Нет записей',
      })
    );
  }

  load();
}