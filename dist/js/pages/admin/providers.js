// Провайдеры, модели, discovery и назначения ролей (llm/stt/tts)

import { h } from '../../core/dom.js';
import { request, ApiError } from '../../core/api.js';
import {
  table, badge, field, modal, confirmModal, toast, skeleton, emptyState, tabs,
} from '../../core/components.js';

const KINDS = [
  { value: 'openai_compatible', label: 'OpenAI-совместимый' },
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'gemini', label: 'Google Gemini' },
  { value: 'elevenlabs', label: 'ElevenLabs' },
  { value: 'deepgram', label: 'Deepgram' },
  { value: 'local', label: 'Локальный' },
];

const MODEL_ROLES = [
  { value: 'llm', title: 'Диалог (LLM)', desc: 'Модель, разговаривающая с пользователем от лица собеседника' },
  { value: 'stt', title: 'Распознавание речи (STT)', desc: 'Превращает речь пользователя в текст' },
  { value: 'tts', title: 'Синтез речи (TTS)', desc: 'Озвучивает реплики собеседника' },
];

function errMsg(err, fallback) {
  return err instanceof ApiError ? err.message : fallback;
}

function kindBadge(kind) {
  const meta = KINDS.find((k) => k.value === kind);
  return badge(meta ? meta.label : kind, 'info');
}

function roleBadge(role) {
  const meta = MODEL_ROLES.find((r) => r.value === role);
  return badge(role, role === 'llm' ? 'accent' : role === 'stt' ? 'info' : 'warning');
}

function boolBadge(v, yes, no) {
  return v ? badge(yes, 'success') : badge(no, 'danger');
}

function formModal({ title, body, submitLabel = 'Сохранить', wide = false, onSubmit }) {
  let m;
  const cancel = h('button.btn.btn-secondary', { type: 'button', text: 'Отмена' });
  const save = h('button.btn.btn-primary', { type: 'button', text: submitLabel });
  m = modal({ title, body, actions: [cancel, save] });
  if (wide) m.dialog.style.width = 'min(640px, 100%)';
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
  let providers = [];
  let models = [];
  let assignments = [];
  let providerFilter = ''; // '' = все

  const providersHost = h('div');
  const modelsHost = h('div');
  const assignHost = h('div');

  const panels = { providers: providersHost, models: modelsHost, assign: assignHost };
  const tabsEl = tabs({
    items: [
      { id: 'providers', label: 'Провайдеры' },
      { id: 'models', label: 'Модели' },
      { id: 'assign', label: 'Назначения ролей' },
    ],
    active: 'providers',
    onChange: (id) => {
      for (const [key, el] of Object.entries(panels)) {
        el.classList.toggle('hidden', key !== id);
      }
    },
  });

  root.replaceChildren(
    h('div.page-header', null,
      h('div', null,
        h('h1', null, 'Провайдеры'),
        h('div.page-sub', null, 'API-ключи, модели и назначения по ролям')
      )
    ),
    tabsEl,
    h('div.mt-4', null, providersHost, modelsHost, assignHost)
  );

  modelsHost.classList.add('hidden');
  assignHost.classList.add('hidden');

  providersHost.append(skeleton(3));
  modelsHost.append(skeleton(3));
  assignHost.append(skeleton(3));

  async function loadAll() {
    try {
      providers = await request('/providers');
      renderProviders();
    } catch (err) {
      providersHost.replaceChildren(
        h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить провайдеров') })
      );
      toast(errMsg(err, 'Не удалось загрузить провайдеров'), 'error');
    }
    try {
      models = await request('/models');
      renderModels();
    } catch (err) {
      modelsHost.replaceChildren(
        h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить модели') })
      );
    }
    try {
      assignments = await request('/model-assignments');
      renderAssignments();
    } catch (err) {
      assignHost.replaceChildren(
        h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить назначения') })
      );
    }
  }

  async function loadAssignments() {
    try {
      assignments = await request('/model-assignments');
      renderAssignments();
    } catch (err) {
      toast(errMsg(err, 'Не удалось обновить назначения'), 'error');
    }
  }

  async function loadModels() {
    try {
      models = await request('/models', providerFilter ? { query: { provider_id: providerFilter } } : {});
      renderModels();
    } catch (err) {
      toast(errMsg(err, 'Не удалось обновить модели'), 'error');
    }
  }

  // ── Провайдеры ──

  function renderProviders() {
    const addBtn = h('button.btn.btn-primary', { type: 'button', text: '+ Добавить провайдера' });
    addBtn.addEventListener('click', () => openProviderEditor(null));

    if (!providers.length) {
      providersHost.replaceChildren(
        h('div.card', null,
          h('div.card-header', null,
            h('div.card-title', { text: 'Провайдеры' }),
            addBtn
          ),
          h('div.card-body', null,
            emptyState({
              icon: '⚙',
              title: 'Провайдеров нет',
              description: 'Добавьте провайдера ИИ (OpenAI-совместимый, Anthropic, локальный и т.д.).',
            })
          )
        )
      );
      return;
    }

    const grid = h('div.grid-2');
    for (const p of providers) grid.append(providerCard(p));

    providersHost.replaceChildren(
      h('div.card', null,
        h('div.card-header', null,
          h('div.card-title', { text: 'Провайдеры' }),
          addBtn
        ),
        h('div.card-body', null, grid)
      )
    );
  }

  function providerCard(p) {
    const pingBtn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Ping' });
    pingBtn.addEventListener('click', async () => {
      pingBtn.disabled = true;
      try {
        await request(`/providers/${p.id}/ping`, { method: 'POST' });
        toast(`${p.name}: соединение в порядке`, 'success');
      } catch (err) {
        toast(`${p.name}: ${errMsg(err, 'ping не прошёл')}`, 'error');
      } finally {
        pingBtn.disabled = false;
      }
    });

    const discBtn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Discovery' });
    discBtn.addEventListener('click', () => openDiscovery(p));

    const editBtn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Изменить' });
    editBtn.addEventListener('click', () => openProviderEditor(p));

    const delBtn = h('button.btn.btn-danger.btn-sm', { type: 'button', text: 'Удалить' });
    delBtn.addEventListener('click', async () => {
      const ok = await confirmModal({
        title: 'Удалить провайдера?',
        message: `«${p.name}» и его настройки будут удалены.`,
        confirmText: 'Удалить',
        danger: true,
      });
      if (!ok) return;
      try {
        await request(`/providers/${p.id}`, { method: 'DELETE' });
        toast('Провайдер удалён', 'success');
        await loadAll();
      } catch (err) {
        toast(errMsg(err, 'Не удалось удалить провайдера'), 'error');
      }
    });

    return h('div.card', null,
      h('div.card-header', null,
        h('div.card-title', { text: p.name }),
        p.is_enabled ? badge('включён', 'success') : badge('выключен', 'danger')
      ),
      h('div.card-body', null,
        h('div.row', null,
          kindBadge(p.kind),
          h('span.small.muted', { text: p.base_url || 'базовый URL не указан' })
        ),
        h('div.small.muted.mt-3', {
          text: `API-ключ: ${p.api_key_hint || 'не задан'}`,
          style: { fontFamily: 'var(--font-mono)' },
        })
      ),
      h('div.card-footer', null, pingBtn, discBtn, editBtn, delBtn)
    );
  }

  function openProviderEditor(p) {
    const isEdit = Boolean(p);
    const nameF = field({ label: 'Название', required: true, value: p?.name || '', placeholder: 'OpenRouter' });
    const kindF = field({
      label: 'Тип', type: 'select', value: p?.kind || 'openai_compatible',
      options: KINDS,
    });
    const urlF = field({ label: 'Base URL', value: p?.base_url || '', placeholder: 'https://api.example.com/v1' });
    const keyF = field({
      label: 'API-ключ', type: 'password',
      hint: isEdit
        ? 'Оставьте пустым — ключ не изменится. Ключ шифруется и хранится на сервере.'
        : 'Обязателен для облачных провайдеров (кроме «Локальный»).',
    });
    const enabledF = field({
      label: 'Включён', type: 'select',
      value: p?.is_enabled === false ? '0' : '1',
      options: [{ value: '1', label: 'Да' }, { value: '0', label: 'Нет' }],
    });

    formModal({
      title: isEdit ? `Провайдер — ${p.name}` : 'Новый провайдер',
      body: h('div.stack', null, nameF, kindF, urlF, keyF, enabledF),
      onSubmit: async () => {
        nameF.setError('');
        const name = nameF.control.value.trim();
        if (!name) { nameF.setError('Укажите название'); return false; }
        const apiKey = keyF.control.value;
        const body = {
          id: p?.id || '',
          name,
          kind: kindF.control.value,
          base_url: urlF.control.value.trim() || null,
          is_enabled: enabledF.control.value === '1',
          created_at: p?.created_at || '',
          updated_at: p?.updated_at ?? null,
        };
        // Пустой ключ = не менять: поле не попадает в тело запроса
        if (apiKey) body.api_key = apiKey;

        await request('/providers', { method: 'POST', body });
        toast(isEdit ? 'Провайдер сохранён' : 'Провайдер добавлен', 'success');
        await loadAll();
        return true;
      },
    });
  }

  function openDiscovery(provider) {
    const roleF = field({
      label: 'Роль модели', type: 'select', value: 'llm',
      options: MODEL_ROLES.map((r) => ({ value: r.value, label: r.title })),
    });
    const listHost = h('div.stack', null, h('div.small.muted', { text: 'Выберите роль и нажмите «Найти модели».' }));
    let m;

    const findBtn = h('button.btn.btn-secondary', { type: 'button', text: 'Найти модели' });
    findBtn.addEventListener('click', async () => {
      if (findBtn.disabled) return;
      findBtn.disabled = true;
      findBtn.textContent = 'Ищем…';
      listHost.replaceChildren(skeleton(3));
      try {
        const found = await request(`/providers/${provider.id}/discover`, {
          query: { role: roleF.control.value },
        });
        if (!found.length) {
          listHost.replaceChildren(h('div.small.muted', { text: 'Модели не найдены.' }));
          return;
        }
        listHost.replaceChildren();
        for (const d of found) {
          const addBtn = h('button.btn.btn-primary.btn-sm', { type: 'button', text: 'Добавить' });
          addBtn.addEventListener('click', async () => {
            addBtn.disabled = true;
            try {
              await request('/models', {
                method: 'POST',
                body: {
                  id: '',
                  provider_id: provider.id,
                  role: d.role,
                  model_key: d.model_key,
                  display_name: d.display_name || d.model_key,
                  is_enabled: true,
                  metadata: d.notes ? { notes: d.notes } : {},
                  created_at: '',
                },
              });
              addBtn.textContent = 'Добавлено ✓';
              toast(`Модель ${d.display_name || d.model_key} добавлена`, 'success');
              await loadModels();
            } catch (err) {
              addBtn.disabled = false;
              toast(errMsg(err, 'Не удалось добавить модель'), 'error');
            }
          });
          listHost.append(
            h('div.row-between', { style: { padding: '6px 0', borderBottom: '1px solid var(--border)' } },
              h('div', null,
                h('div', { text: d.display_name || d.model_key }),
                h('div.small.muted', { text: d.model_key })
              ),
              h('div.row', { style: { gap: '6px' } },
                roleBadge(d.role),
                addBtn
              )
            )
          );
        }
      } catch (err) {
        listHost.replaceChildren(h('div.text-danger', { text: errMsg(err, 'Discovery не удался') }));
        toast(errMsg(err, 'Discovery не удался'), 'error');
      } finally {
        findBtn.disabled = false;
        findBtn.textContent = 'Найти модели';
      }
    });

    const closeBtn = h('button.btn.btn-secondary', { type: 'button', text: 'Закрыть' });
    m = modal({
      title: `Discovery — ${provider.name}`,
      body: h('div.stack', null, roleF, h('div', null, findBtn), listHost),
      actions: [closeBtn],
    });
    closeBtn.addEventListener('click', () => m.close());
  }

  // ── Модели ──

  function renderModels() {
    const filterSel = field({
      label: 'Провайдер', type: 'select', value: providerFilter,
      options: [
        { value: '', label: 'Все провайдеры' },
        ...providers.map((p) => ({ value: p.id, label: p.name })),
      ],
    });
    filterSel.control.addEventListener('change', () => {
      providerFilter = filterSel.control.value;
      loadModels();
    });

    const addBtn = h('button.btn.btn-primary', { type: 'button', text: '+ Добавить модель' });
    addBtn.addEventListener('click', () => openModelEditor(null));

    const allModels = models;
    const rows = providerFilter ? allModels.filter((m) => m.provider_id === providerFilter) : allModels;

    const body = rows.length
      ? table({
          columns: [
            { key: 'display_name', label: 'Название' },
            { key: 'model_key', label: 'Ключ', mono: true },
            { key: 'role', label: 'Роль', render: (m) => roleBadge(m.role) },
            { key: 'is_enabled', label: 'Статус', render: (m) => boolBadge(m.is_enabled, 'включена', 'выключена') },
            {
              key: 'provider_id',
              label: 'Провайдер',
              render: (m) => {
                const p = providers.find((x) => x.id === m.provider_id);
                return p ? p.name : m.provider_id;
              },
            },
            { key: 'actions', label: 'Действия', render: (m) => modelActions(m) },
          ],
          rows,
          emptyText: 'Нет моделей',
        })
      : emptyState({
          icon: '◈',
          title: 'Моделей нет',
          description: 'Добавьте модель вручную или найдите через Discovery у провайдера.',
        });

    modelsHost.replaceChildren(
      h('div.card', null,
        h('div.card-header', null,
          h('div.card-title', { text: 'Модели' }),
          h('div.row', null, filterSel, addBtn)
        ),
        h('div.card-body', null, body)
      )
    );
  }

  function modelActions(m) {
    const editBtn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Изменить' });
    editBtn.addEventListener('click', (e) => { e.stopPropagation(); openModelEditor(m); });

    const delBtn = h('button.btn.btn-danger.btn-sm', { type: 'button', text: 'Удалить' });
    delBtn.addEventListener('click', async (e) => {
      e.stopPropagation();
      const ok = await confirmModal({
        title: 'Удалить модель?',
        message: `«${m.display_name}» (${m.model_key}) будет удалена.`,
        confirmText: 'Удалить',
        danger: true,
      });
      if (!ok) return;
      try {
        await request(`/models/${m.id}`, { method: 'DELETE' });
        toast('Модель удалена', 'success');
        await loadModels();
        await loadAssignments();
      } catch (err) {
        toast(errMsg(err, 'Не удалось удалить модель'), 'error');
      }
    });

    return h('div.row', { style: { gap: '6px' } }, editBtn, delBtn);
  }

  function openModelEditor(m) {
    if (!providers.length) {
      toast('Сначала добавьте провайдера', 'warning');
      return;
    }
    const isEdit = Boolean(m);
    const providerF = field({
      label: 'Провайдер', type: 'select', value: m?.provider_id || providers[0].id,
      options: providers.map((p) => ({ value: p.id, label: p.name })),
    });
    const roleF = field({
      label: 'Роль', type: 'select', value: m?.role || 'llm',
      options: MODEL_ROLES.map((r) => ({ value: r.value, label: r.title })),
    });
    const keyF = field({ label: 'Ключ модели', required: true, value: m?.model_key || '', placeholder: 'gpt-4o-mini' });
    const nameF = field({ label: 'Отображаемое имя', value: m?.display_name || '', hint: 'Пусто — будет равно ключу' });
    const enabledF = field({
      label: 'Включена', type: 'select',
      value: m?.is_enabled === false ? '0' : '1',
      options: [{ value: '1', label: 'Да' }, { value: '0', label: 'Нет' }],
    });
    const metaF = field({
      label: 'Metadata (JSON, необязательно)', type: 'textarea', rows: 3,
      value: m?.metadata && Object.keys(m.metadata).length ? JSON.stringify(m.metadata, null, 2) : '',
      hint: 'Например: {"notes": "голоса", "context": "128k"}',
    });

    formModal({
      title: isEdit ? `Модель — ${m.display_name}` : 'Новая модель',
      body: h('div.stack', null, providerF, roleF, keyF, nameF, enabledF, metaF),
      onSubmit: async () => {
        keyF.setError('');
        const model_key = keyF.control.value.trim();
        if (!model_key) { keyF.setError('Укажите ключ модели'); return false; }

        let metadata = {};
        const rawMeta = metaF.control.value.trim();
        if (rawMeta) {
          try {
            metadata = JSON.parse(rawMeta);
          } catch {
            metaF.setError('Некорректный JSON');
            return false;
          }
          metaF.setError('');
        }

        await request('/models', {
          method: 'POST',
          body: {
            id: m?.id || '',
            provider_id: providerF.control.value,
            role: roleF.control.value,
            model_key,
            display_name: nameF.control.value.trim(),
            is_enabled: enabledF.control.value === '1',
            metadata,
            created_at: m?.created_at || '',
          },
        });
        toast(isEdit ? 'Модель сохранена' : 'Модель добавлена', 'success');
        await loadModels();
        await loadAssignments();
        return true;
      },
    });
  }

  // ── Назначения ролей ──

  function renderAssignments() {
    const cards = h('div.stack');
    for (const role of MODEL_ROLES) {
      cards.append(assignmentCard(role));
    }
    assignHost.replaceChildren(
      h('div.card', null,
        h('div.card-header', null,
          h('div.card-title', { text: 'Назначения ролей' }),
          h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Обновить', onClick: () => loadAssignments() })
        ),
        h('div.card-body', null, cards)
      )
    );
  }

  function assignmentCard(role) {
    const roleModels = models.filter((m) => m.role === role.value);
    const current = assignments.find((a) => a.role === role.value);
    const currentModel = current ? models.find((m) => m.id === current.model_id) : null;

    const sel = field({
      label: 'Модель для роли', type: 'select',
      value: current?.model_id || '',
      options: [
        { value: '', label: roleModels.length ? '— не назначена —' : 'нет моделей этой роли' },
        ...roleModels.map((m) => ({
          value: m.id,
          label: `${m.display_name || m.model_key} (${m.model_key})${m.is_enabled ? '' : ' — выключена'}`,
        })),
      ],
      disabled: !roleModels.length,
    });

    const saveBtn = h('button.btn.btn-primary.btn-sm', { type: 'button', text: 'Сохранить' });
    saveBtn.addEventListener('click', async () => {
      const model_id = sel.control.value;
      if (!model_id) { toast('Выберите модель', 'warning'); return; }
      saveBtn.disabled = true;
      try {
        await request(`/model-assignments/${role.value}`, { method: 'PUT', body: { model_id } });
        toast(`Роль «${role.title}» назначена`, 'success');
        await loadAssignments();
      } catch (err) {
        toast(errMsg(err, 'Не удалось назначить модель'), 'error');
      } finally {
        saveBtn.disabled = false;
      }
    });

    const clearBtn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Снять назначение' });
    clearBtn.disabled = !current;
    clearBtn.addEventListener('click', async () => {
      const ok = await confirmModal({
        title: 'Снять назначение?',
        message: `Роль «${role.title}» останется без модели, пока не будет назначена новая. Функции с этой ролью будут недоступны (503).`,
        confirmText: 'Снять',
        danger: true,
      });
      if (!ok) return;
      clearBtn.disabled = true;
      try {
        await request(`/model-assignments/${role.value}`, { method: 'DELETE' });
        toast(`Назначение роли «${role.title}» снято`, 'success');
        await loadAssignments();
      } catch (err) {
        toast(errMsg(err, 'Не удалось снять назначение'), 'error');
        clearBtn.disabled = false;
      }
    });

    return h('div.card', { style: { background: 'var(--surface-2)' } },
      h('div.card-body', null,
        h('div.row-between', null,
          h('div', null,
            h('div', { style: { fontWeight: '700' }, text: role.title }),
            h('div.small.muted', { text: role.desc })
          ),
          roleBadge(role.value)
        ),
        h('div.small.muted.mt-3', {
          text: currentModel
            ? `Сейчас: ${currentModel.display_name || currentModel.model_key}`
            : 'Сейчас: не назначена',
        }),
        h('div.row.mt-3', null, sel, saveBtn, clearBtn)
      )
    );
  }

  loadAll();
}
