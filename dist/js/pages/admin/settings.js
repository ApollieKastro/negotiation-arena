// Настройки платформы: глобальные ключи, типизированные формы, ссылка на аудит

import { h } from '../../core/dom.js';
import { request, ApiError } from '../../core/api.js';
import { table, field, modal, toast, skeleton, emptyState } from '../../core/components.js';

function errMsg(err, fallback) {
  return err instanceof ApiError ? err.message : fallback;
}

/** Подсказки и типы для известных ключей глобальных настройок. */
const KNOWN_KEYS = {
  'platform.site_name': {
    hint: 'Название площадки в шапке и заголовках.',
    control: 'text',
  },
  'platform.default_theme': {
    hint: 'Тема по умолчанию для новых пользователей.',
    control: 'select',
    options: [
      { value: 'light', label: 'Светлая' },
      { value: 'dark', label: 'Тёмная' },
      { value: 'system', label: 'Системная' },
    ],
  },
  'platform.default_font_size': {
    hint: 'Размер шрифта по умолчанию.',
    control: 'select',
    options: [
      { value: 'sm', label: 'Маленький (sm)' },
      { value: 'md', label: 'Средний (md)' },
      { value: 'lg', label: 'Большой (lg)' },
    ],
  },
  'platform.default_locale': {
    hint: 'Язык интерфейса по умолчанию.',
    control: 'select',
    options: [{ value: 'ru', label: 'Русский (ru)' }],
  },
  'platform.max_turns': {
    hint: 'Максимум ходов в сессии (число).',
    control: 'number',
  },
  'platform.llm_daily_token_limit': {
    hint: 'Дневной лимит LLM-токенов на пользователя; 0 = лимит выключен, при исчерпании — 429.',
    control: 'number',
  },
  'scoring.llm_judge_enabled': {
    hint: 'LLM-судья: оценивать реплики игрока моделью поверх эвристики. Выключено — баллы только по ключевым словам.',
    control: 'select',
    options: [
      { value: 'true', label: 'Включён' },
      { value: 'false', label: 'Выключен' },
    ],
  },
  'scoring.llm_judge_weight': {
    hint: 'Доля LLM-оценки в баллах хода, от 0 до 1 (0.4 = 40% модель / 60% эвристика).',
    control: 'number',
  },
};

function formModal({ title, body, submitLabel = 'Сохранить', onSubmit }) {
  let m;
  const cancel = h('button.btn.btn-secondary', { type: 'button', text: 'Отмена' });
  const save = h('button.btn.btn-primary', { type: 'button', text: submitLabel });
  m = modal({ title, body, actions: [cancel, save] });
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

/** Контракт страницы: рендер в root (DOM-контейнер) + params роутера */
export function renderPage(root, params = {}) {
  let settings = {};
  let loaded = false;

  const tableHost = h('div');

  const addBtn = h('button.btn.btn-primary', { type: 'button', text: '+ Добавить ключ' });
  addBtn.addEventListener('click', () => openEditor(null));

  const auditLink = h('a.btn.btn-secondary', { href: '#/admin/audit' }, 'Журнал аудита →');

  root.replaceChildren(
    h('div.page-header', null,
      h('div', null,
        h('h1', null, 'Настройки платформы'),
        h('div.page-sub', null, 'Глобальные ключи: название, тема, шрифт, лимиты')
      ),
      h('div.page-actions', null, addBtn, auditLink)
    ),
    h('div.card', null,
      h('div.card-header', null,
        h('div.card-title', { text: 'Ключи' }),
        h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Обновить', onClick: () => load() })
      ),
      tableHost
    ),
    h('div.card.mt-4', null,
      h('div.card-header', null, h('div.card-title', { text: 'Аудит' })),
      h('div.card-body', null,
        h('div.row-between', null,
          h('div.small.muted', { text: 'Журнал действий администраторов и пользователей.' }),
          h('a.btn.btn-secondary.btn-sm', { href: '#/admin/audit' }, 'Открыть аудит')
        )
      )
    )
  );

  tableHost.append(skeleton(5));

  async function load() {
    try {
      settings = await request('/settings/global');
      loaded = true;
      renderTable();
    } catch (err) {
      tableHost.replaceChildren(
        h('div.card-body', null, h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить настройки') }))
      );
      toast(errMsg(err, 'Не удалось загрузить настройки'), 'error');
    }
  }

  function renderTable() {
    if (!loaded) return;
    const keys = Object.keys(settings).sort();
    if (!keys.length) {
      tableHost.replaceChildren(
        emptyState({ icon: '☰', title: 'Настроек нет', description: 'Добавьте первый ключ.' })
      );
      return;
    }

    tableHost.replaceChildren(
      table({
        columns: [
          { key: 'key', label: 'Ключ', render: (r) => h('code', { text: r.key }) },
          {
            key: 'value',
            label: 'Значение',
            render: (r) => h('span', { text: String(r.value), style: { fontFamily: 'var(--font-mono)' } }),
          },
          {
            key: 'hint',
            label: 'Подсказка',
            render: (r) => {
              const meta = KNOWN_KEYS[r.key];
              return h('span.small.muted', { text: meta ? meta.hint : '—' });
            },
          },
          {
            key: 'actions',
            label: '',
            render: (r) => {
              const btn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Изменить' });
              btn.addEventListener('click', (e) => { e.stopPropagation(); openEditor(r.key); });
              return btn;
            },
          },
        ],
        rows: keys.map((key) => ({ key, value: settings[key] })),
        emptyText: 'Нет ключей',
      })
    );
  }

  function buildControl(key, value) {
    const meta = KNOWN_KEYS[key];
    if (!meta) {
      const f = field({
        label: 'Значение',
        value: String(value ?? ''),
        hint: 'Неизвестный ключ — значение сохраняется как текст.',
      });
      return { f, read: () => f.control.value };
    }
    if (meta.control === 'select') {
      const f = field({
        label: 'Значение', type: 'select', value: String(value ?? ''),
        options: meta.options, hint: meta.hint,
      });
      return { f, read: () => f.control.value };
    }
    if (meta.control === 'number') {
      const f = field({
        label: 'Значение', type: 'number', value: String(value ?? ''), hint: meta.hint,
      });
      return {
        f,
        read: () => {
          const n = Number(f.control.value);
          return Number.isFinite(n) ? String(n) : f.control.value.trim();
        },
      };
    }
    const f = field({ label: 'Значение', value: String(value ?? ''), hint: meta.hint });
    return { f, read: () => f.control.value };
  }

  function openEditor(key) {
    let keyF = null;
    let ctrl;
    if (key === null) {
      keyF = field({
        label: 'Ключ', required: true, placeholder: 'platform.site_name',
        hint: 'Известные: platform.site_name, platform.default_theme, platform.default_font_size, platform.default_locale, platform.max_turns, platform.llm_daily_token_limit, scoring.llm_judge_enabled, scoring.llm_judge_weight',
      });
      ctrl = buildControl('', '');
    } else {
      ctrl = buildControl(key, settings[key]);
    }

    formModal({
      title: key === null ? 'Добавить ключ' : `Изменить — ${key}`,
      body: h('div.stack', null, keyF, ctrl.f),
      submitLabel: key === null ? 'Добавить' : 'Сохранить',
      onSubmit: async () => {
        const finalKey = key === null ? keyF.control.value.trim() : key;
        if (!finalKey) { keyF.setError('Укажите ключ'); return false; }
        if (finalKey.startsWith('user:')) {
          keyF.setError('Пользовательские ключи нельзя писать как глобальные');
          return false;
        }
        const value = ctrl.read();
        await request('/settings/global', { method: 'PUT', body: { key: finalKey, value } });
        toast('Настройка сохранена', 'success');
        await load();
        return true;
      },
    });
  }

  load();
}
