// Переиспользуемые UI-компоненты: toast, modal, field, table и др.

import { h, escapeHtml } from './dom.js';
import { fetchAvatarUrl } from './avatars.js';

const TOAST_ICONS = { info: 'ℹ', success: '✓', error: '✕', warning: '!' };

/** Тост: toast('Сохранено', 'success') */
export function toast(message, type = 'info', duration = 3500) {
  const root = document.getElementById('toast-root');
  if (!root) return;
  const el = h(
    `div.toast.toast-${type}`,
    { role: 'status' },
    h('span.toast-icon', { text: TOAST_ICONS[type] || TOAST_ICONS.info }),
    h('span', { text: String(message ?? '') }),
    h('button.toast-close', { type: 'button', 'aria-label': 'Закрыть', text: '×' })
  );
  const remove = () => {
    if (!el.isConnected) return;
    el.classList.add('is-leaving');
    el.addEventListener('animationend', () => el.remove(), { once: true });
    setTimeout(() => el.remove(), 250);
  };
  el.querySelector('.toast-close').addEventListener('click', remove);
  root.append(el);
  if (duration > 0) setTimeout(remove, duration);
  return { close: remove, el };
}

/**
 * Модалка: modal({title, body, actions}) → {el, close}
 * Закрытие: Esc, клик по оверлею, вызов close().
 */
export function modal({ title, body, actions = [], onClose } = {}) {
  const overlay = h('div.modal-overlay', { role: 'presentation' });
  const titleId = `modal-title-${Date.now()}`;
  const dialog = h(
    'div.modal',
    { role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': titleId }
  );

  const closeBtn = h('button.modal-close', { type: 'button', 'aria-label': 'Закрыть', text: '×' });
  const header = h(
    'div.modal-header',
    null,
    h('div.modal-title', { id: titleId, text: title || '' }),
    closeBtn
  );

  const bodyNode =
    body instanceof Node
      ? body
      : h('div.modal-body-text', { text: String(body ?? '') });
  const bodyEl = h('div.modal-body', null, bodyNode);

  const actionsEl = h('div.modal-actions');
  for (const action of actions) {
    if (action instanceof Node) { actionsEl.append(action); continue; }
    const btn = h('button', {
      type: 'button',
      class: `btn ${action.variant ? `btn-${action.variant}` : 'btn-secondary'}`,
      text: action.label || 'OK',
      disabled: action.disabled || false,
    });
    if (typeof action.onClick === 'function') {
      btn.addEventListener('click', async () => {
        const res = action.onClick();
        if (res !== false) close();
      });
    }
    actionsEl.append(btn);
  }

  dialog.append(header, bodyEl);
  if (actionsEl.childNodes.length) dialog.append(actionsEl);
  overlay.append(dialog);

  let closed = false;
  function close() {
    if (closed) return;
    closed = true;
    document.removeEventListener('keydown', onKey);
    overlay.remove();
    document.body.style.removeProperty('overflow');
    if (typeof onClose === 'function') onClose();
  }
  function onKey(e) { if (e.key === 'Escape') { e.stopPropagation(); close(); } }

  closeBtn.addEventListener('click', close);
  overlay.addEventListener('mousedown', (e) => { if (e.target === overlay) close(); });
  document.addEventListener('keydown', onKey);
  document.body.style.overflow = 'hidden';
  document.body.append(overlay);

  return { el: overlay, close, dialog };
}

/** confirmModal({...}) → Promise<boolean> */
export function confirmModal({
  title = 'Подтвердите действие',
  message = '',
  confirmText = 'Подтвердить',
  cancelText = 'Отмена',
  danger = false,
} = {}) {
  return new Promise((resolve) => {
    let settled = false;
    const settle = (value) => {
      if (settled) return;
      settled = true;
      resolve(value);
    };
    const m = modal({
      title,
      body: h('p', { text: message }),
      actions: [
        { label: cancelText, variant: 'secondary', onClick: () => settle(false) },
        { label: confirmText, variant: danger ? 'danger' : 'primary', onClick: () => settle(true) },
      ],
      onClose: () => settle(false),
    });
    void m;
  });
}

/**
 * Поле формы: field({label, type, name, value, ...}) → label.element
 * el.control — элемент ввода; el.setError(msg) — показать/скрыть ошибку.
 */
export function field({
  label = '',
  type = 'text',
  name = '',
  value = '',
  placeholder = '',
  required = false,
  autocomplete,
  hint = '',
  rows = 4,
  options = null,
  disabled = false,
} = {}) {
  const el = h('label.field');
  if (label) {
    el.append(
      h('span.field-label', null, label, required ? h('span.req', { text: '*' }) : null)
    );
  }

  let control;
  if (type === 'textarea') {
    control = h('textarea.input', { name, placeholder, rows, disabled });
    control.value = value ?? '';
  } else if (type === 'select') {
    control = h('select.input', { name, disabled });
    for (const opt of options || []) {
      const o = h('option', { value: opt.value, text: opt.label ?? String(opt.value) });
      if (String(opt.value) === String(value ?? '')) o.selected = true;
      control.append(o);
    }
  } else {
    control = h('input.input', {
      type,
      name,
      placeholder,
      required: required || null,
      autocomplete: autocomplete || null,
      disabled: disabled || null,
    });
    control.value = value ?? '';
  }

  const errorEl = h('span.field-error');
  el.append(control, errorEl);
  if (hint) el.append(h('span.field-hint', { text: hint }));

  // У <label> есть нативный геттер control — перекрываем own-свойством
  Object.defineProperty(el, 'control', { value: control, configurable: true, writable: true });
  el.setError = (msg) => {
    errorEl.textContent = msg || '';
    control.classList.toggle('is-invalid', Boolean(msg));
    if (msg) control.setAttribute('aria-invalid', 'true');
    else control.removeAttribute('aria-invalid');
  };
  return el;
}

/** Бейдж: badge('admin', 'accent') */
export function badge(text, variant = 'neutral', { dot = true } = {}) {
  return h(`span.badge.badge-${variant}${dot ? '' : '.no-dot'}`, { text: String(text ?? '') });
}

/**
 * Таблица: table({columns:[{key,label,render?,align?}], rows, onRowClick})
 * columns.key — поле строки; render(row) — кастомная ячейка.
 */
export function table({ columns = [], rows = [], onRowClick, emptyText = 'Нет данных' } = {}) {
  const wrap = h('div.table-wrap');
  const thead = h('thead');
  const headRow = h('tr');
  for (const col of columns) {
    headRow.append(
      h(`th${col.align === 'right' ? '.align-right' : ''}`, { text: col.label || col.key })
    );
  }
  thead.append(headRow);

  const tbody = h('tbody');
  if (!rows.length) {
    tbody.append(h('tr', null, h('td', { colspan: String(columns.length || 1) },
      h('span.muted', { text: emptyText }))));
  } else {
    for (const row of rows) {
      const tr = h('tr', { class: onRowClick ? 'is-clickable' : null });
      if (onRowClick) tr.addEventListener('click', (e) => onRowClick(row, e));
      for (const col of columns) {
        const content = typeof col.render === 'function' ? col.render(row) : row[col.key];
        const td = h(`td${col.align === 'right' ? '.align-right' : ''}`);
        if (content instanceof Node) td.append(content);
        else td.innerHTML = content === null || content === undefined || content === ''
          ? '<span class="muted">—</span>'
          : escapeHtml(content);
        if (col.mono) td.classList.add('num');
        tr.append(td);
      }
      tbody.append(tr);
    }
  }

  const tbl = h('table.table', null, thead, tbody);
  wrap.append(tbl);
  return wrap;
}

/** Скелетон: skeleton(3) — N строк; skeleton({width,height}) — один блок */
export function skeleton(countOrOpts = 3, height = 16) {
  if (typeof countOrOpts === 'object' && countOrOpts) {
    const { width = '100%', height: hh = height } = countOrOpts;
    return h('div.skeleton', { style: { width: String(width), height: String(hh) } });
  }
  const n = Number(countOrOpts) || 3;
  const stack = h('div.skeleton-stack');
  for (let i = 0; i < n; i++) {
    const w = i === n - 1 ? '60%' : '100%';
    stack.append(h('div.skeleton', { style: { width: w, height: `${height}px` } }));
  }
  return stack;
}

/** Пустое состояние: emptyState({icon,title,description,action}) */
export function emptyState({ icon = '∅', title = 'Пока пусто', description = '', action = null } = {}) {
  return h(
    'div.empty-state',
    null,
    h('div.empty-state-icon', { text: icon }),
    h('div.empty-state-title', { text: title }),
    description ? h('div.empty-state-desc', { text: description }) : null,
    action instanceof Node ? action : null
  );
}

/** Stat-карточка: statCard({label, value, hint, trend}) */
export function statCard({ label = '', value = '—', hint = '', trend = null } = {}) {
  const hintClass = trend === 'up' ? 'stat-hint up' : trend === 'down' ? 'stat-hint down' : 'stat-hint';
  return h(
    'div.stat-card',
    null,
    h('div.stat-label', { text: label }),
    h('div.stat-value', { text: String(value) }),
    hint ? h('div', { class: hintClass, text: hint }) : null
  );
}

/** Спиннер: spinner('lg') или spinner({label}) → блок по центру */
export function spinner(size = '', { label = '' } = {}) {
  if (!label && !size) return h('span.spinner');
  if (typeof size === 'object' && size) {
    return spinner('', size);
  }
  const cls = size === 'sm' ? 'spinner spinner-sm' : size === 'lg' ? 'spinner spinner-lg' : 'spinner';
  if (!label) return h('span', { class: cls });
  return h('div.spinner-block', null, h('span', { class: cls }), h('span', { text: label }));
}

/** Табы: tabs({items:[{id,label}], active, onChange}) → el с el.setActive(id) */
export function tabs({ items = [], active, onChange, variant = '' } = {}) {
  let current = active ?? items[0]?.id;
  const el = h('div', { class: `tabs${variant ? ` tabs-${variant}` : ''}`, role: 'tablist' });

  function render() {
    el.replaceChildren();
    for (const item of items) {
      const btn = h('button', {
        type: 'button',
        role: 'tab',
        class: `tab${item.id === current ? ' is-active' : ''}`,
        'aria-selected': item.id === current ? 'true' : 'false',
        text: item.label,
      });
      btn.addEventListener('click', () => {
        if (current === item.id) return;
        current = item.id;
        render();
        if (typeof onChange === 'function') onChange(current);
      });
      el.append(btn);
    }
  }

  el.setActive = (id) => { current = id; render(); };
  el.getActive = () => current;
  render();
  return el;
}

/** Аватар юзера: avatar(user, 'sm') — картинка (has_avatar) или инициалы */
export function avatar(u, size = '') {
  const cls = size === 'sm' ? 'avatar avatar-sm' : size === 'lg' ? 'avatar avatar-lg' : 'avatar';
  const name = (u && (u.display_name || u.login)) || '?';
  const initials = name
    .split(/\s+/)
    .slice(0, 2)
    .map((p) => p[0])
    .join('')
    .toUpperCase();
  const el = h('span', {
    class: `${cls} avatar-initials`,
    title: name,
    text: initials,
  });

  if (u && u.has_avatar && u.id) {
    fetchAvatarUrl(u.id).then((url) => {
      // Страница могла успеть перерисоваться — не трогаем оторванный узел.
      if (!url || !el.isConnected) return;
      el.replaceChildren(
        h('img.avatar-img', { src: url, alt: '', decoding: 'async' })
      );
      el.classList.remove('avatar-initials');
      el.classList.add('has-image');
    });
  }
  return el;
}

/**
 * Секция-карточка на странице настроек/профиля:
 * sectionCard('Заголовок', 'Описание', ...children).
 */
export function sectionCard(title, description, ...children) {
  return h('div.card.mt-4', null,
    h('div.card-body.stack', { style: { gap: 'var(--sp-4)' } },
      h('div.stack', { style: { gap: '2px' } },
        h('div.card-title', { text: title }),
        description ? h('div.small.muted', { text: description }) : null
      ),
      ...children
    )
  );
}
