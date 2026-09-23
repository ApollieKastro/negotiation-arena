// Настройки: внешний вид, модели, звук, профиль

import { h } from '../core/dom.js';
import { field, toast, spinner, emptyState } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import * as store from '../core/store.js';

const THEME_OPTIONS = [
  { value: 'system', label: 'Системная' },
  { value: 'light', label: 'Светлая' },
  { value: 'dark', label: 'Тёмная' },
];
const FONT_OPTIONS = [
  { value: 'sm', label: 'Маленький (sm)' },
  { value: 'md', label: 'Средний (md)' },
  { value: 'lg', label: 'Большой (lg)' },
];
const MODEL_ROLES = [
  { role: 'llm', label: 'Диалог (LLM)' },
  { role: 'tts', label: 'Синтез речи (TTS)' },
  { role: 'stt', label: 'Распознавание речи (STT)' },
];

function sectionCard(title, description, ...children) {
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

export function renderPage(root, params = {}) {
  const u = store.user() || {};
  // Guard от гонки: не перерисовываем, если ушли со страницы
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'settings' });
  const isCurrent = () => marker.isConnected;

  // ── Внешний вид ──
  const themeF = field({
    label: 'Тема оформления',
    type: 'select',
    value: store.getTheme(),
    options: THEME_OPTIONS,
    hint: '«Системная» следует за настройками ОС. Синхронизируется с аккаунтом.',
  });
  themeF.control.addEventListener('change', () => {
    store.setTheme(themeF.control.value);
    toast('Тема обновлена', 'success', 2000);
    updatePreview();
  });

  const fontF = field({
    label: 'Размер шрифта',
    type: 'select',
    value: store.getFontSize(),
    options: FONT_OPTIONS,
    hint: 'Применяется ко всему интерфейсу.',
  });
  fontF.control.addEventListener('change', () => {
    store.setFontSize(fontF.control.value);
    toast('Размер шрифта обновлён', 'success', 2000);
    updatePreview();
  });

  const preview = h('div.card', {
    style: { background: 'var(--surface-2)', border: '1px dashed var(--border-strong)' },
  },
    h('div.card-body.stack', { style: { gap: 'var(--sp-2)' } },
      h('div.small.muted', { text: 'Превью' }),
      h('div', { id: 'preview-text', text: 'Привет! Обсудим условия поставки и найдём решение, выгодное обеим сторонам.' }),
      h('div.small', { id: 'preview-sub', text: 'Вторичный текст — подписи и подсказки.' })
    )
  );
  function updatePreview() {
    preview.querySelector('#preview-text').style.fontWeight = '600';
    preview.querySelector('#preview-sub').style.color = 'var(--muted)';
  }

  // ── Звук ──
  let soundOn = String(store.getSettings().sound_enabled ?? 'true') !== 'false';
  const soundBtn = h('button.btn.btn-secondary.btn-sm', {
    type: 'button',
    'aria-pressed': soundOn ? 'true' : 'false',
  });
  function renderSoundBtn() {
    soundBtn.textContent = soundOn ? 'Включён ✓' : 'Выключен ✕';
    soundBtn.setAttribute('aria-pressed', soundOn ? 'true' : 'false');
  }
  renderSoundBtn();
  soundBtn.addEventListener('click', async () => {
    const next = !soundOn;
    soundBtn.disabled = true;
    try {
      await store.setSetting('sound_enabled', next ? 'true' : 'false');
      soundOn = next;
      renderSoundBtn();
      toast(next ? 'Звук включён' : 'Звук выключен', 'success', 2000);
    } catch (err) {
      toast(err instanceof ApiError ? err.message : 'Не удалось сохранить настройку', 'error');
    } finally {
      soundBtn.disabled = false;
    }
  });

  // ── Профиль (read-only) ──
  const loginF = field({
    label: 'Логин',
    value: u.login || '',
    disabled: true,
    hint: 'Изменение логина на бэке недоступно — placeholder.',
  });
  const nameF = field({
    label: 'Отображаемое имя',
    value: u.display_name || '',
    placeholder: 'Не задано',
    disabled: true,
    hint: 'Редактирование имени появится позже — API профиля пока не поддерживает.',
  });

  // ── Модели ──
  const modelsHost = h('div.stack');
  modelsHost.append(spinner('', { label: 'Загружаем модели…' }));

  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: 'Настройки' }),
        h('div.page-sub', null, 'Внешний вид, модели, звук и профиль')
      )
    ),

    sectionCard('Внешний вид', 'Тема и размер шрифта сохраняются в аккаунте.',
      themeF, fontF, preview),

    sectionCard('Модели', 'Своя модель вместо глобального назначения роли.',
      modelsHost),

    sectionCard('Звук', 'Сигналы и эффекты интерфейса.',
      h('div.row-between', null,
        h('div.small.muted', { text: 'Звуковые эффекты' }),
        soundBtn
      )),

    sectionCard('Профиль', 'Данные учётной записи.',
      loginF, nameF)
  );

  updatePreview();

  // ── Загрузка моделей ──
  Promise.all([
    request('/model-preferences/options', { query: { role: 'llm' } }).catch(() => []),
    request('/model-preferences/options', { query: { role: 'tts' } }).catch(() => []),
    request('/model-preferences/options', { query: { role: 'stt' } }).catch(() => []),
    request('/model-preferences/me').catch(() => []),
  ]).then(([llmOpts, ttsOpts, sttOpts, prefs]) => {
    if (!isCurrent()) return;
    const optionsByRole = { llm: llmOpts || [], tts: ttsOpts || [], stt: sttOpts || [] };
    const prefList = Array.isArray(prefs) ? prefs : [];
    const anyOptions = Object.values(optionsByRole).some((arr) => arr.length);

    if (!anyOptions && !prefList.length) {
      modelsHost.replaceChildren(h('div.small.muted', {
        text: 'Модели не назначены администратором — выбор недоступен.',
      }));
      return;
    }

    modelsHost.replaceChildren();

    for (const { role, label } of MODEL_ROLES) {
      const opts = optionsByRole[role];
      const current = prefList.find((p) => p.role === role) || null;
      if (!opts.length && !current) continue;

      const currentLine = h('div.small', null,
        h('span.muted', { text: 'Текущий выбор: ' }),
        current
          ? h('strong', { text: current.model_display_name || current.model_id })
          : h('span', { text: 'глобальная модель по умолчанию' })
      );

      const resetBtn = h('button.btn.btn-ghost.btn-sm', {
        type: 'button',
        text: 'Сбросить на глобальную',
        disabled: !current,
        onClick: async () => {
          resetBtn.disabled = true;
          try {
            await request(`/model-preferences/me/${role}`, { method: 'DELETE' });
            toast('Возвращена глобальная модель', 'success');
            if (isCurrent()) renderPage(root, params);
          } catch (err) {
            toast(err instanceof ApiError ? err.message : 'Не удалось сбросить', 'error');
            resetBtn.disabled = false;
          }
        },
      });

      let selectF;
      if (opts.length) {
        selectF = field({
          label: `Своя модель вместо глобальной — ${label}`,
          type: 'select',
          value: current ? current.model_id : '',
          options: [
            { value: '', label: 'Глобальная (по умолчанию)' },
            ...opts.map((m) => ({ value: m.id, label: m.display_name || m.model_key })),
          ],
          hint: current
            ? `Выбрано: ${current.model_display_name || current.model_id}`
            : 'Используется назначение администратора.',
        });
        selectF.control.addEventListener('change', async () => {
          const modelId = selectF.control.value;
          selectF.setError('');
          try {
            if (!modelId) {
              await request(`/model-preferences/me/${role}`, { method: 'DELETE' });
              toast('Возвращена глобальная модель', 'success');
            } else {
              await request(`/model-preferences/me/${role}`, {
                method: 'PUT',
                body: { model_id: modelId },
              });
              toast('Модель сохранена', 'success');
            }
            if (isCurrent()) renderPage(root, params);
          } catch (err) {
            selectF.setError(err instanceof ApiError ? err.message : 'Не удалось сохранить');
            toast(err instanceof ApiError ? err.message : 'Ошибка сохранения', 'error');
          }
        });
      }

      modelsHost.append(
        h('div.card', { style: { background: 'var(--surface-2)' } },
          h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
            h('div.card-title', { text: label }),
            currentLine,
            selectF || h('div.small.muted', { text: 'Свободный выбор моделей не назначен.' }),
            h('div.row', null, resetBtn)
          )
        )
      );
    }

    if (!modelsHost.childNodes.length) {
      modelsHost.replaceChildren(h('div.small.muted', {
        text: 'Модели не назначены администратором — выбор недоступен.',
      }));
    }
  }).catch(() => {
    if (!isCurrent()) return;
    modelsHost.replaceChildren(emptyState({
      icon: '⚙',
      title: 'Не удалось загрузить модели',
      description: 'Проверьте соединение и попробуйте позже.',
    }));
  });
}
