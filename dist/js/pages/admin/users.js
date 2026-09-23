// Пользователи: список, роли, блокировка, добавление, статистика

import { h, fmtDateTime } from '../../core/dom.js';
import { request, ApiError } from '../../core/api.js';
import { table, badge, field, modal, confirmModal, toast, skeleton, emptyState, statCard } from '../../core/components.js';
import { user as currentUser } from '../../core/store.js';

function errMsg(err, fallback) {
  return err instanceof ApiError ? err.message : fallback;
}

/** Форма-модалка: кнопки-узлы, close() сам управляет закрытием. */
function formModal({ title, body, submitLabel = 'Сохранить', wide = false, onSubmit }) {
  let m;
  const cancel = h('button.btn.btn-secondary', { type: 'button', text: 'Отмена' });
  const save = h('button.btn.btn-primary', { type: 'button', text: submitLabel });
  m = modal({ title, body, actions: [cancel, save] });
  if (wide) m.dialog.style.width = 'min(720px, 100%)';
  cancel.addEventListener('click', () => m.close());
  save.addEventListener('click', async () => {
    if (save.disabled) return;
    save.disabled = true;
    const label = save.textContent;
    save.textContent = 'Сохраняем…';
    try {
      const res = await onSubmit();
      if (res !== false) m.close();
    } catch (err) {
      toast(errMsg(err, 'Не удалось сохранить'), 'error');
    } finally {
      save.disabled = false;
      save.textContent = label;
    }
  });
  return m;
}

function roleBadge(role) {
  return role === 'admin'
    ? badge('админ', 'accent')
    : badge('пользователь', 'neutral');
}

function activeBadge(isActive) {
  return isActive ? badge('активен', 'success') : badge('отключён', 'danger');
}

/** Контракт страницы: рендер в root (DOM-контейнер) + params роутера */
export function renderPage(root, params = {}) {
  let users = [];
  let filter = '';
  let loaded = false;

  const tableHost = h('div');
  const countHost = h('span.small.muted');

  const search = h('input.input', {
    type: 'search',
    placeholder: 'Поиск по логину…',
    style: { maxWidth: '260px', minWidth: '180px' },
  });
  search.addEventListener('input', () => {
    filter = search.value.trim().toLowerCase();
    renderTable();
  });

  const addBtn = h('button.btn.btn-primary', { type: 'button', text: '+ Добавить пользователя' });
  addBtn.addEventListener('click', () => openAddUser());

  root.replaceChildren(
    h('div.page-header', null,
      h('div', null,
        h('h1', null, 'Пользователи'),
        h('div.page-sub', null, 'Роли, активность и учётные записи')
      ),
      h('div.page-actions', null, addBtn)
    ),
    h('div.card', null,
      h('div.card-header', null,
        h('div.row', null, search, countHost),
        h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Обновить', onClick: () => load() })
      ),
      tableHost
    )
  );

  tableHost.append(skeleton(6));

  async function load() {
    try {
      users = await request('/users');
      loaded = true;
      renderTable();
    } catch (err) {
      tableHost.replaceChildren(
        h('div.card-body', null, h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить пользователей') }))
      );
      toast(errMsg(err, 'Не удалось загрузить пользователей'), 'error');
    }
  }

  function renderTable() {
    if (!loaded) return;
    const q = filter;
    const rows = q ? users.filter((u) => u.login.toLowerCase().includes(q)) : users;
    countHost.textContent = `${rows.length} из ${users.length}`;

    if (!users.length) {
      tableHost.replaceChildren(
        emptyState({ icon: '☺', title: 'Пользователей нет', description: 'Создайте первую учётную запись.' })
      );
      return;
    }

    tableHost.replaceChildren(
      table({
        columns: [
          { key: 'login', label: 'Логин' },
          { key: 'display_name', label: 'Имя' },
          { key: 'role', label: 'Роль', render: (u) => roleBadge(u.role) },
          { key: 'is_active', label: 'Статус', render: (u) => activeBadge(u.is_active) },
          { key: 'created_at', label: 'Создан', render: (u) => fmtDateTime(u.created_at) },
          { key: 'actions', label: 'Действия', render: (u) => actionsCell(u) },
        ],
        rows,
        emptyText: 'Нет пользователей по фильтру',
      })
    );
  }

  function actionsCell(u) {
    const me = currentUser();
    const isSelf = me && me.id === u.id;

    const statsBtn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Статистика' });
    statsBtn.addEventListener('click', (e) => { e.stopPropagation(); showStats(u); });

    const roleBtn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Роль' });
    roleBtn.addEventListener('click', (e) => { e.stopPropagation(); changeRole(u); });

    const toggleBtn = h('button.btn.btn-ghost.btn-sm', {
      type: 'button',
      text: u.is_active ? 'Отключить' : 'Включить',
      disabled: isSelf && u.is_active ? true : null,
      title: isSelf && u.is_active ? 'Нельзя отключить самого себя' : '',
    });
    toggleBtn.addEventListener('click', (e) => { e.stopPropagation(); toggleActive(u); });

    const delBtn = h('button.btn.btn-danger.btn-sm', { type: 'button', text: 'Удалить' });
    delBtn.addEventListener('click', (e) => { e.stopPropagation(); removeUser(u); });

    return h('div.row', { style: { gap: '6px' } }, statsBtn, roleBtn, toggleBtn, delBtn);
  }

  async function changeRole(u) {
    const roleF = field({
      label: 'Роль',
      type: 'select',
      value: u.role,
      options: [
        { value: 'user', label: 'Пользователь' },
        { value: 'admin', label: 'Администратор' },
      ],
      hint: 'Роль применяется сразу, без повторного входа.',
    });
    const me = currentUser();
    if (me && me.id === u.id) {
      roleF.append(h('span.field-hint', { text: 'Свою роль изменить нельзя.', style: { color: 'var(--danger)' } }));
    }

    formModal({
      title: `Роль — ${u.login}`,
      body: h('div.stack', null, roleF),
      submitLabel: 'Применить',
      onSubmit: async () => {
        const role = roleF.control.value;
        if (role === u.role) return true;
        await request(`/users/${u.id}/role`, { method: 'PATCH', body: { role } });
        toast('Роль обновлена', 'success');
        await load();
        return true;
      },
    });
  }

  async function toggleActive(u) {
    const next = !u.is_active;
    if (!next) {
      const ok = await confirmModal({
        title: 'Отключить учётную запись?',
        message: `${u.login} не сможет войти, активные сессии будут невалидны.`,
        confirmText: 'Отключить',
        danger: true,
      });
      if (!ok) return;
    }
    try {
      await request(`/users/${u.id}/active`, { method: 'PATCH', body: { is_active: next } });
      toast(next ? 'Учётная запись включена' : 'Учётная запись отключена', 'success');
      await load();
    } catch (err) {
      toast(errMsg(err, 'Не удалось изменить активность'), 'error');
    }
  }

  async function removeUser(u) {
    const ok = await confirmModal({
      title: 'Удалить пользователя?',
      message: `Учётная запись «${u.login}» будет удалена безвозвратно.`,
      confirmText: 'Удалить',
      danger: true,
    });
    if (!ok) return;
    try {
      await request(`/users/${u.id}`, { method: 'DELETE' });
      toast('Пользователь удалён', 'success');
      await load();
    } catch (err) {
      toast(errMsg(err, 'Не удалось удалить пользователя'), 'error');
    }
  }

  function openAddUser() {
    const loginF = field({
      label: 'Логин', required: true, placeholder: 'alice',
      hint: 'Латиница, цифры, _ - . ; минимум 3 символа',
    });
    const passF = field({
      label: 'Пароль', type: 'password', required: true, hint: 'Минимум 6 символов',
    });
    const roleF = field({
      label: 'Роль', type: 'select',
      options: [
        { value: 'user', label: 'Пользователь' },
        { value: 'admin', label: 'Администратор' },
      ],
    });
    const nameF = field({ label: 'Имя (необязательно)', placeholder: 'Алиса' });

    formModal({
      title: 'Добавить пользователя',
      body: h('div.stack', null, loginF, passF, roleF, nameF),
      submitLabel: 'Создать',
      onSubmit: async () => {
        loginF.setError('');
        passF.setError('');
        const login = loginF.control.value.trim();
        const password = passF.control.value;
        if (login.length < 3) { loginF.setError('Логин — минимум 3 символа'); return false; }
        if (password.length < 6) { passF.setError('Пароль — минимум 6 символов'); return false; }
        const display_name = nameF.control.value.trim();
        await request('/users', {
          method: 'POST',
          body: {
            login,
            password,
            role: roleF.control.value,
            ...(display_name ? { display_name } : {}),
          },
        });
        toast(`Пользователь ${login} создан`, 'success');
        await load();
        return true;
      },
    });
  }

  async function showStats(u) {
    const body = h('div.stack', null, skeleton(3));
    modal({ title: `Статистика — ${u.login}`, body });
    try {
      const s = await request(`/stats/users/${u.id}`);
      body.replaceChildren(
        h('div.stat-grid', { style: { marginBottom: '0' } },
          statCard({ label: 'Всего сессий', value: s.total_sessions }),
          statCard({ label: 'Завершено', value: s.finished }),
          statCard({ label: 'Активных', value: s.active }),
          statCard({ label: 'Брошено', value: s.abandoned }),
          statCard({ label: 'Лучший балл', value: s.best_score }),
          statCard({ label: 'Средний балл', value: s.avg_score })
        ),
        h('div.small.muted', {
          text: `Последняя сессия: ${s.last_session_at ? fmtDateTime(s.last_session_at) : '—'}`,
        }),
        s.display_name ? h('div.small.muted', { text: `Имя: ${s.display_name}` }) : null
      );
    } catch (err) {
      body.replaceChildren(h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить статистику') }));
      toast(errMsg(err, 'Не удалось загрузить статистику'), 'error');
    }
  }

  load();
}
