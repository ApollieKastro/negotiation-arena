// DOM-утилиты: hyperscript, монтирование, форматирование (ru)

/** Создаёт элемент: h('div.cls#id', props, ...children) */
export function h(tag, props, ...children) {
  const { name, classes, id } = parseTag(tag);
  const el = document.createElement(name);
  if (classes.length) el.className = classes.join(' ');
  if (id) el.id = id;
  if (props && (typeof props !== 'object' || Array.isArray(props) || props instanceof Node)) {
    children.unshift(props);
  } else if (props) {
    applyProps(el, props);
  }
  appendChildren(el, children);
  return el;
}

/** Разбирает 'button.btn.btn-primary#save' → {name, classes, id} */
function parseTag(tag) {
  const s = String(tag);
  const name = s.split(/[.#]/)[0] || 'div';
  const classes = [];
  let id = '';
  const re = /[.#]([^.#]+)/g;
  let m;
  while ((m = re.exec(s))) {
    if (s[m.index] === '#') id = m[1];
    else classes.push(m[1]);
  }
  return { name, classes, id };
}

function applyProps(el, props) {
  for (const [key, value] of Object.entries(props)) {
    if (value === null || value === undefined || value === false) continue;
    if (key === 'class' || key === 'className') {
      const extra = Array.isArray(value) ? value.filter(Boolean).join(' ') : String(value);
      el.className = el.className ? `${el.className} ${extra}` : extra;
    } else if (key === 'text') {
      el.textContent = String(value);
    } else if (key === 'html') {
      el.innerHTML = String(value);
    } else if (key === 'style' && typeof value === 'object') {
      Object.assign(el.style, value);
    } else if (key === 'dataset' && typeof value === 'object') {
      Object.assign(el.dataset, value);
    } else if (key.startsWith('on') && typeof value === 'function') {
      el.addEventListener(key.slice(2).toLowerCase(), value);
    } else if (key === 'value' || key === 'checked' || key === 'disabled') {
      el[key] = value;
    } else {
      el.setAttribute(key, value === true ? '' : String(value));
    }
  }
}

function appendChildren(el, children) {
  for (const child of children.flat(Infinity)) {
    if (child === null || child === undefined || child === false || child === true) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
}

/** Очищает контейнер и монтирует элемент */
export function mount(el, container) {
  clear(container);
  container.append(el);
  return el;
}

/** Очищает контейнер */
export function clear(container) {
  if (container) container.replaceChildren();
}

/** Дата: ДД.ММ.ГГГГ */
export function fmtDate(value) {
  const d = toDate(value);
  if (!d) return '—';
  return new Intl.DateTimeFormat('ru-RU', { day: '2-digit', month: '2-digit', year: 'numeric' }).format(d);
}

/** Дата и время: ДД.ММ.ГГГГ, ЧЧ:ММ */
export function fmtDateTime(value) {
  const d = toDate(value);
  if (!d) return '—';
  return new Intl.DateTimeFormat('ru-RU', {
    day: '2-digit', month: '2-digit', year: 'numeric',
    hour: '2-digit', minute: '2-digit',
  }).format(d);
}

/** Длительность в секундах → «1 ч 05 мин» / «2 мин 30 с» */
export function fmtDuration(seconds) {
  const s = Math.max(0, Math.round(Number(seconds) || 0));
  if (!seconds && seconds !== 0) return '—';
  const hPart = Math.floor(s / 3600);
  const mPart = Math.floor((s % 3600) / 60);
  const sPart = s % 60;
  const pad = (n) => String(n).padStart(2, '0');
  if (hPart > 0) return `${hPart} ч ${pad(mPart)} мин`;
  if (mPart > 0) return `${mPart} мин ${pad(sPart)} с`;
  return `${sPart} с`;
}

/** Русские склонения: plural(n, ['сессия','сессии','сессий']) */
export function plural(n, forms) {
  const abs = Math.abs(Math.trunc(Number(n) || 0)) % 100;
  const n1 = abs % 10;
  if (abs > 10 && abs < 20) return forms[2];
  if (n1 > 1 && n1 < 5) return forms[1];
  if (n1 === 1) return forms[0];
  return forms[2];
}

/** Экранирование HTML */
export function escapeHtml(value) {
  return String(value ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function toDate(value) {
  if (value === null || value === undefined || value === '') return null;
  const d = value instanceof Date ? value : new Date(value);
  return Number.isNaN(d.getTime()) ? null : d;
}
