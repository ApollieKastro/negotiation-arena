// Настройки: внешний вид, язык, модели, звук, профиль

import { h } from '../core/dom.js';
import { field, toast, spinner, emptyState } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import * as store from '../core/store.js';
import { t, getLocale, LOCALES } from '../core/i18n.js';

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
    label: t('settings.theme'),
    type: 'select',
    value: store.getTheme(),
    options: [
      { value: 'system', label: t('settings.themeSystem') },
      { value: 'light', label: t('settings.themeLight') },
      { value: 'dark', label: t('settings.themeDark') },
    ],
    hint: t('settings.themeHint'),
  });
  themeF.control.addEventListener('change', () => {
    store.setTheme(themeF.control.value);
    toast(t('settings.themeUpdated'), 'success', 2000);
    updatePreview();
  });

  const fontF = field({
    label: t('settings.fontSize'),
    type: 'select',
    value: store.getFontSize(),
    options: [
      { value: 'sm', label: t('settings.fontSm') },
      { value: 'md', label: t('settings.fontMd') },
      { value: 'lg', label: t('settings.fontLg') },
    ],
    hint: t('settings.fontSizeHint'),
  });
  fontF.control.addEventListener('change', () => {
    store.setFontSize(fontF.control.value);
    toast(t('settings.fontUpdated'), 'success', 2000);
    updatePreview();
  });

  // ── Язык ──
  const langF = field({
    label: t('settings.language'),
    type: 'select',
    value: getLocale(),
    options: LOCALES.map((l) => ({ value: l.value, label: t(l.labelKey) })),
    hint: t('settings.languageHint'),
  });
  langF.control.addEventListener('change', async () => {
    const locale = langF.control.value;
    try {
      await store.setLocale(locale);
      toast(t('settings.languageUpdated'), 'success', 2000);
      // main.js слушает locale-changed и перерисует shell + страницу
    } catch {
      toast(t('settings.saveFail'), 'error');
    }
  });

  const preview = h('div.card', {
    style: { background: 'var(--surface-2)', border: '1px dashed var(--border-strong)' } },
    h('div.card-body.stack', { style: { gap: 'var(--sp-2)' } },
      h('div.small.muted', { text: t('settings.preview') }),
      h('div', { id: 'preview-text', text: t('settings.previewText') }),
      h('div.small', { id: 'preview-sub', text: t('settings.previewSub') })
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
    soundBtn.textContent = soundOn ? t('settings.soundOn') : t('settings.soundOff');
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
      toast(next ? t('settings.soundEnabledToast') : t('settings.soundDisabledToast'), 'success', 2000);
    } catch (err) {
      toast(err instanceof ApiError ? err.message : t('settings.saveFail'), 'error');
    } finally {
      soundBtn.disabled = false;
    }
  });

  // ── Профиль (read-only) ──
  const loginF = field({
    label: t('settings.login'),
    value: u.login || '',
    disabled: true,
    hint: t('settings.loginHint'),
  });
  const nameF = field({
    label: t('settings.displayName'),
    value: u.display_name || '',
    placeholder: t('settings.displayNamePlaceholder'),
    disabled: true,
    hint: t('settings.displayNameHint'),
  });

  // ── Модели ──
  const modelsHost = h('div.stack');
  modelsHost.append(spinner('', { label: t('settings.modelsLoading') }));

  const MODEL_ROLES = [
    { role: 'llm', label: t('settings.modelRole.llm') },
    { role: 'tts', label: t('settings.modelRole.tts') },
    { role: 'stt', label: t('settings.modelRole.stt') },
  ];

  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: t('settings.title') }),
        h('div.page-sub', null, t('settings.sub'))
      )
    ),

    sectionCard(t('settings.appearance'), t('settings.appearanceDesc'),
      themeF, fontF, langF, preview),

    sectionCard(t('settings.models'), t('settings.modelsDesc'),
      modelsHost),

    sectionCard(t('settings.sound'), t('settings.soundDesc'),
      h('div.row-between', null,
        h('div.small.muted', { text: t('settings.soundEffects') }),
        soundBtn
      )),

    sectionCard(t('settings.profile'), t('settings.profileDesc'),
      loginF, nameF),

    sectionCard(t('team.title'), t('team.settingsDesc'),
      h('div.row', null,
        h('a.btn.btn-secondary.btn-sm', { href: '#/settings/team' }, t('team.open'))
      ))
  );

  updatePreview();

  // ── Загрузка моделей ──
  // Эффективная модель = та, которой человек реально пользуется в диалоге:
  // личное предпочтение → иначе общее назначение администратора → не настроено.
  const isAdmin = store.isAdmin();
  Promise.all([
    request('/model-preferences/options', { query: { role: 'llm' } }).catch(() => []),
    request('/model-preferences/options', { query: { role: 'tts' } }).catch(() => []),
    request('/model-preferences/options', { query: { role: 'stt' } }).catch(() => []),
    request('/model-preferences/me').catch(() => []),
    request('/model-preferences/effective').catch(() => []),
    isAdmin ? request('/model-assignments').catch(() => []) : Promise.resolve([]),
  ]).then(([llmOpts, ttsOpts, sttOpts, prefs, effective, assignments]) => {
    if (!isCurrent()) return;
    const optionsByRole = { llm: llmOpts || [], tts: ttsOpts || [], stt: sttOpts || [] };
    const prefList = Array.isArray(prefs) ? prefs : [];
    const effList = Array.isArray(effective) ? effective : [];
    const assignList = Array.isArray(assignments) ? assignments : [];
    const anyOptions = Object.values(optionsByRole).some((arr) => arr.length);

    if (!anyOptions && !prefList.length && !effList.some((e) => e && e.display_name)) {
      modelsHost.replaceChildren(h('div.small.muted', {
        text: t('settings.modelsEmpty'),
      }));
      return;
    }

    modelsHost.replaceChildren();

    for (const { role, label } of MODEL_ROLES) {
      const opts = optionsByRole[role];
      const current = prefList.find((p) => p.role === role) || null;
      const eff = effList.find((e) => e && e.role === role) || null;
      const globalId = (assignList.find((a) => a.role === role) || {}).model_id || '';

      // Что реально подключено у этого пользователя — название модели.
      const connected = Boolean(eff && eff.display_name);
      const sourceKey = !connected
        ? 'settings.sourceNone'
        : eff.source === 'personal'
          ? 'settings.sourcePersonal'
          : 'settings.sourceGlobal';
      const effLine = h('div.small', null,
        h('span.muted', { text: t('settings.effectiveLine') }),
        h('strong', { text: connected ? eff.display_name : t('settings.sourceNone') }),
        connected ? h('span.muted', { text: ` · ${t(sourceKey)}` }) : null
      );

      // Админ здесь же редактирует ОБЩЕЕ назначение — им пользуются все,
      // кто не выбрал модель лично. Личное предпочтение админа после
      // смены общего сбрасывается, чтобы не маскировало назначение.
      const cardChildren = [effLine];
      if (isAdmin) {
        const globalF = field({
          label: t('settings.globalModelLabel', { role: label }),
          type: 'select',
          value: globalId,
          options: [
            {
              value: '',
              label: opts.length ? t('settings.globalUnassigned') : t('settings.noRoleModels'),
            },
            ...opts.map((m) => ({ value: m.id, label: m.display_name || m.model_key })),
          ],
          hint: t('settings.globalModelHint'),
        });
        globalF.control.addEventListener('change', async () => {
          const modelId = globalF.control.value;
          globalF.setError('');
          globalF.control.disabled = true;
          try {
            if (modelId) {
              await request(`/model-assignments/${role}`, {
                method: 'PUT',
                body: { model_id: modelId },
              });
            } else {
              await request(`/model-assignments/${role}`, { method: 'DELETE' });
            }
            if (current) {
              await request(`/model-preferences/me/${role}`, { method: 'DELETE' })
                .catch(() => {});
            }
            toast(t('settings.globalSaved'), 'success');
            if (isCurrent()) renderPage(root, params);
          } catch (err) {
            globalF.setError(err instanceof ApiError ? err.message : t('settings.saveFail'));
            toast(err instanceof ApiError ? err.message : t('settings.saveError'), 'error');
            if (isCurrent()) renderPage(root, params);
          } finally {
            globalF.control.disabled = false;
          }
        });
        cardChildren.push(globalF);
      }

      const resetBtn = h('button.btn.btn-ghost.btn-sm', {
        type: 'button',
        text: t('settings.resetGlobal'),
        disabled: !current,
        onClick: async () => {
          resetBtn.disabled = true;
          try {
            await request(`/model-preferences/me/${role}`, { method: 'DELETE' });
            toast(t('settings.globalRestored'), 'success');
            if (isCurrent()) renderPage(root, params);
          } catch (err) {
            toast(err instanceof ApiError ? err.message : t('settings.resetFail'), 'error');
            resetBtn.disabled = false;
          }
        },
      });

      let selectF;
      if (opts.length) {
        selectF = field({
          label: t('settings.customModelLabel', { role: label }),
          type: 'select',
          value: current ? current.model_id : '',
          options: [
            { value: '', label: t('settings.globalOption') },
            ...opts.map((m) => ({ value: m.id, label: m.display_name || m.model_key })),
          ],
          hint: current
            ? t('settings.assignedTo', { name: current.model_display_name || current.model_id })
            : t('settings.useAdminAssign'),
        });
        selectF.control.addEventListener('change', async () => {
          const modelId = selectF.control.value;
          selectF.setError('');
          try {
            if (!modelId) {
              await request(`/model-preferences/me/${role}`, { method: 'DELETE' });
              toast(t('settings.globalRestored'), 'success');
            } else {
              await request(`/model-preferences/me/${role}`, {
                method: 'PUT',
                body: { model_id: modelId },
              });
              toast(t('settings.modelSaved'), 'success');
            }
            if (isCurrent()) renderPage(root, params);
          } catch (err) {
            selectF.setError(err instanceof ApiError ? err.message : t('settings.modelSaveFail'));
            toast(err instanceof ApiError ? err.message : t('settings.saveError'), 'error');
          }
        });
      }

      modelsHost.append(
        h('div.card', { style: { background: 'var(--surface-2)' } },
          h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
            h('div.card-title', { text: label }),
            ...cardChildren,
            selectF || h('div.small.muted', { text: t('settings.freePick') }),
            h('div.row', null, resetBtn)
          )
        )
      );
    }

    if (!modelsHost.childNodes.length) {
      modelsHost.replaceChildren(h('div.small.muted', {
        text: t('settings.modelsEmpty'),
      }));
    }
  }).catch(() => {
    if (!isCurrent()) return;
    modelsHost.replaceChildren(emptyState({
      icon: '⚙',
      title: t('settings.modelsError'),
      description: t('settings.modelsErrorDesc'),
    }));
  });
}
