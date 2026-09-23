// Сценарии: таблица, конструктор, импорт/экспорт, ИИ-генератор

import { h, fmtDateTime } from '../../core/dom.js';
import { request, ApiError } from '../../core/api.js';
import {
  table, badge, field, modal, confirmModal, toast, skeleton, emptyState,
} from '../../core/components.js';

const DIFFICULTIES = [
  { value: 'easy', label: 'Начальная', variant: 'success' },
  { value: 'medium', label: 'Средняя', variant: 'warning' },
  { value: 'hard', label: 'Сложная', variant: 'danger' },
];

function errMsg(err, fallback) {
  return err instanceof ApiError ? err.message : fallback;
}

function difficultyBadge(d) {
  const meta = DIFFICULTIES.find((x) => x.value === d) || DIFFICULTIES[1];
  return badge(meta.label, meta.variant);
}

function formModal({ title, body, submitLabel = 'Сохранить', wide = false, onSubmit }) {
  let m;
  const cancel = h('button.btn.btn-secondary', { type: 'button', text: 'Отмена' });
  const save = h('button.btn.btn-primary', { type: 'button', text: submitLabel });
  m = modal({ title, body, actions: [cancel, save] });
  if (wide) m.dialog.style.width = 'min(760px, 100%)';
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

function downloadJson(data, filename) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = h('a', { href: url, download: filename });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** Контракт страницы: рендер в root (DOM-контейнер) + params роутера */
export function renderPage(root, params = {}) {
  let scenarios = [];
  let loaded = false;

  const tableHost = h('div');

  const createBtn = h('button.btn.btn-primary', { type: 'button', text: '+ Создать' });
  createBtn.addEventListener('click', () => openEditor(null));

  const aiBtn = h('button.btn.btn-secondary', { type: 'button', text: '✨ ИИ-генератор' });
  aiBtn.addEventListener('click', () => openGenerator());

  const fileInput = h('input', { type: 'file', accept: '.json,application/json', style: { display: 'none' } });
  const importBtn = h('button.btn.btn-secondary', { type: 'button', text: 'Импорт JSON' });
  importBtn.addEventListener('click', () => fileInput.click());
  fileInput.addEventListener('change', () => {
    const file = fileInput.files && fileInput.files[0];
    fileInput.value = '';
    if (file) importFile(file);
  });

  root.replaceChildren(
    h('div.page-header', null,
      h('div', null,
        h('h1', null, 'Сценарии'),
        h('div.page-sub', null, 'Конструктор, публикация, импорт/экспорт и ИИ-генерация')
      ),
      h('div.page-actions', null, createBtn, aiBtn, importBtn, fileInput)
    ),
    h('div.card', null,
      h('div.card-header', null,
        h('div.card-title', { text: 'Все сценарии' }),
        h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Обновить', onClick: () => load() })
      ),
      tableHost
    )
  );

  tableHost.append(skeleton(6));

  async function load() {
    try {
      scenarios = await request('/scenarios');
      loaded = true;
      renderTable();
    } catch (err) {
      tableHost.replaceChildren(
        h('div.card-body', null, h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить сценарии') }))
      );
      toast(errMsg(err, 'Не удалось загрузить сценарии'), 'error');
    }
  }

  function renderTable() {
    if (!loaded) return;
    if (!scenarios.length) {
      tableHost.replaceChildren(
        emptyState({
          icon: '◈',
          title: 'Сценариев нет',
          description: 'Создайте сценарий вручную, импортируйте JSON или сгенерируйте ИИ.',
        })
      );
      return;
    }

    tableHost.replaceChildren(
      table({
        columns: [
          {
            key: 'title',
            label: 'Название',
            render: (s) => h('div', null,
              h('div', { text: s.title }),
              !s.is_active ? h('span.small.muted', { text: 'черновик — не виден игрокам' }) : null
            ),
          },
          { key: 'sphere', label: 'Сфера' },
          { key: 'difficulty', label: 'Сложность', render: (s) => difficultyBadge(s.difficulty) },
          {
            key: 'ai_generated',
            label: 'ИИ',
            render: (s) => (s.ai_generated ? badge('ИИ', 'info') : badge('нет', 'neutral')),
          },
          {
            key: 'is_active',
            label: 'Статус',
            render: (s) => (s.is_active ? badge('активен', 'success') : badge('черновик', 'warning')),
          },
          {
            key: 'updated_at',
            label: 'Обновлён',
            render: (s) => fmtDateTime(s.updated_at || s.created_at),
          },
          { key: 'actions', label: 'Действия', render: (s) => actionsCell(s) },
        ],
        rows: scenarios,
        emptyText: 'Нет сценариев',
      })
    );
  }

  function actionsCell(s) {
    const editBtn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Редактировать' });
    editBtn.addEventListener('click', (e) => { e.stopPropagation(); openEditor(s); });

    const toggleBtn = h('button.btn.btn-ghost.btn-sm', {
      type: 'button',
      text: s.is_active ? 'Снять' : 'Опубликовать',
    });
    toggleBtn.addEventListener('click', (e) => { e.stopPropagation(); toggleActive(s); });

    const expBtn = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Экспорт' });
    expBtn.addEventListener('click', (e) => { e.stopPropagation(); exportScenario(s); });

    const delBtn = h('button.btn.btn-danger.btn-sm', { type: 'button', text: 'Удалить' });
    delBtn.addEventListener('click', (e) => { e.stopPropagation(); removeScenario(s); });

    return h('div.row', { style: { gap: '6px' } }, editBtn, toggleBtn, expBtn, delBtn);
  }

  async function toggleActive(s) {
    try {
      const updated = await request(`/scenarios/${s.id}/active`, {
        method: 'PATCH',
        body: { is_active: !s.is_active },
      });
      const i = scenarios.findIndex((x) => x.id === s.id);
      if (i !== -1) scenarios[i] = updated;
      renderTable();
      toast(updated.is_active ? 'Сценарий опубликован' : 'Сценарий снят с публикации', 'success');
    } catch (err) {
      toast(errMsg(err, 'Не удалось изменить статус'), 'error');
    }
  }

  async function exportScenario(s) {
    try {
      const data = await request(`/scenarios/${s.id}/export`);
      const safe = String(s.title || s.id).replace(/[^\wа-яё\- ]+/gi, '').trim().replace(/\s+/g, '-').slice(0, 60) || s.id;
      downloadJson(data, `scenario-${safe}.json`);
      toast('Файл экспортирован', 'success');
    } catch (err) {
      toast(errMsg(err, 'Не удалось экспортировать сценарий'), 'error');
    }
  }

  async function removeScenario(s) {
    const ok = await confirmModal({
      title: 'Удалить сценарий?',
      message: `«${s.title}» будет удалён. Если с ним есть сессии — сервер ответит отказом.`,
      confirmText: 'Удалить',
      danger: true,
    });
    if (!ok) return;
    try {
      await request(`/scenarios/${s.id}`, { method: 'DELETE' });
      scenarios = scenarios.filter((x) => x.id !== s.id);
      renderTable();
      toast('Сценарий удалён', 'success');
    } catch (err) {
      toast(errMsg(err, 'Не удалось удалить сценарий'), 'error');
    }
  }

  async function importFile(file) {
    let parsed;
    try {
      parsed = JSON.parse(await file.text());
    } catch {
      toast('Файл не является валидным JSON', 'error');
      return;
    }
    try {
      const created = await request('/scenarios/import', { method: 'POST', body: parsed });
      toast(`Сценарий «${created.title}» импортирован`, 'success');
      await load();
    } catch (err) {
      toast(errMsg(err, 'Не удалось импортировать сценарий'), 'error');
    }
  }

  // ── Конструктор ──

  function openEditor(scenario) {
    const isEdit = Boolean(scenario);
    const src = scenario || null;

    const titleF = field({ label: 'Название', required: true, value: src?.title || '', placeholder: 'Аренда офиса' });
    const descF = field({ label: 'Описание', type: 'textarea', rows: 3, value: src?.description || '' });
    const sphereF = field({ label: 'Сфера', value: src?.sphere || '', placeholder: 'Продажи' });
    const diffF = field({
      label: 'Сложность', type: 'select', value: src?.difficulty || 'medium',
      options: DIFFICULTIES.map((d) => ({ value: d.value, label: d.label })),
    });

    const playerRoleF = field({ label: 'Роль игрока', required: true, value: src?.player_role || '' });
    const playerCompanyF = field({ label: 'Компания игрока', value: src?.player_company || '' });
    const playerGoalF = field({ label: 'Цель игрока', required: true, value: src?.player_goal || '' });
    const playerBatnaF = field({ label: 'BATNA игрока', value: src?.player_batna || '' });

    const partnerNameF = field({ label: 'Имя собеседника', required: true, value: src?.partner_name || '' });
    const partnerRoleF = field({ label: 'Должность собеседника', required: true, value: src?.partner_role || '' });
    const partnerCompanyF = field({ label: 'Компания собеседника', value: src?.partner_company || '' });
    const partnerGoalF = field({ label: 'Цель собеседника', required: true, value: src?.partner_goal || '' });
    const partnerGoalsF = field({
      label: 'Все цели собеседника (по строке на цель)',
      type: 'textarea', rows: 3,
      value: (src?.partner_goals || []).join('\n'),
    });
    const partnerBatnaF = field({ label: 'BATNA собеседника', value: src?.partner_batna || '' });

    const toneF = field({ label: 'Тон', value: src?.partner_personality?.tone || '' });
    const styleF = field({ label: 'Манера', value: src?.partner_personality?.style || '' });
    const traitsF = field({ label: 'Черты (через запятую)', value: src?.partner_personality?.traits || '' });

    const openingF = field({
      label: 'Реплика открытия (первая реплика собеседника)',
      required: true, type: 'textarea', rows: 3,
      value: src?.opening_context || '',
    });

    const activeF = field({
      label: 'Опубликован', type: 'select',
      value: src?.is_active ? '1' : '0',
      options: [{ value: '1', label: 'Да — виден игрокам' }, { value: '0', label: 'Нет — черновик' }],
      hint: 'Сценарии, сгенерированные ИИ, создаются как черновики.',
    });

    // ── Динамические финалы ──
    const endingsHost = h('div.stack');
    const endings = [];
    let endingSeq = 0;

    function addEnding(data = {}) {
      endingSeq += 1;
      const row = {
        uid: `end-${Date.now()}-${endingSeq}`,
        id: data.id || `e${endingSeq}`,
        titleF: field({ label: 'Название финала', required: true, value: data.title || '' }),
        minScoreF: field({ label: 'Мин. балл', type: 'number', value: String(data.min_score ?? 0) }),
        textF: field({ label: 'Текст', type: 'textarea', rows: 2, value: data.text || '' }),
        outcomeF: field({ label: 'Исход (для карточки)', value: data.outcome || '' }),
        el: null,
      };
      const delBtn = h('button.btn.btn-danger.btn-sm', { type: 'button', text: 'Удалить финал' });
      delBtn.addEventListener('click', () => {
        const i = endings.indexOf(row);
        if (i !== -1) endings.splice(i, 1);
        row.el.remove();
      });
      row.el = h('div.card', { style: { background: 'var(--surface-2)' } },
        h('div.card-body', null,
          h('div.input-row', null, row.titleF, row.minScoreF),
          row.textF,
          h('div.mt-3', null, row.outcomeF),
          h('div.mt-3', null, delBtn)
        )
      );
      endings.push(row);
      endingsHost.append(row.el);
    }

    const seedEndings = src?.endings?.length
      ? src.endings
      : [
          { id: 'win', title: 'Отличный результат', text: 'Вам удалось договориться на условиях, близких к вашей цели.', outcome: 'Сделка заключена', min_score: 60 },
          { id: 'partial', title: 'Компромисс', text: 'Достигнута частичная договорённость.', outcome: 'Условия согласованы частично', min_score: 25 },
          { id: 'fail', title: 'Переговоры сорвались', text: 'Соглашение достичь не удалось.', outcome: 'Сделки нет', min_score: 0 },
        ];
    for (const e of seedEndings) addEnding(e);

    const addEndingBtn = h('button.btn.btn-secondary.btn-sm', { type: 'button', text: '+ Добавить финал' });
    addEndingBtn.addEventListener('click', () => addEnding());

    const genNote = src?.ai_generated
      ? h('div.field-hint', {
          text: 'Сгенерирован ИИ. Проверьте тексты и опубликуйте («Опубликован: Да»).',
          style: { color: 'var(--warning)' },
        })
      : null;

    const body = h('div.stack', null,
      genNote,
      h('div.card', null, h('div.card-header', null, h('div.card-title', { text: 'Основное' })),
        h('div.card-body', null,
          titleF,
          descF,
          h('div.input-row.mt-3', null, sphereF, diffF)
        )
      ),
      h('div.card', null, h('div.card-header', null, h('div.card-title', { text: 'Игрок' })),
        h('div.card-body', null,
          h('div.input-row', null, playerRoleF, playerCompanyF),
          playerGoalF,
          h('div.mt-3', null, playerBatnaF)
        )
      ),
      h('div.card', null, h('div.card-header', null, h('div.card-title', { text: 'Собеседник' })),
        h('div.card-body', null,
          h('div.input-row', null, partnerNameF, partnerRoleF, partnerCompanyF),
          partnerGoalF,
          h('div.mt-3', null, partnerGoalsF),
          h('div.mt-3', null, partnerBatnaF),
          h('div.input-row.mt-3', null, toneF, styleF, traitsF)
        )
      ),
      h('div.card', null, h('div.card-header', null, h('div.card-title', { text: 'Открытие диалога' })),
        h('div.card-body', null, openingF)
      ),
      h('div.card', null,
        h('div.card-header', null,
          h('div.card-title', { text: 'Финалы' }),
          addEndingBtn
        ),
        h('div.card-body', null, endingsHost)
      ),
      h('div.card', null, h('div.card-header', null, h('div.card-title', { text: 'Публикация' })),
        h('div.card-body', null, activeF)
      )
    );

    formModal({
      title: isEdit ? `Редактирование — ${src.title}` : 'Новый сценарий',
      body,
      wide: true,
      submitLabel: isEdit ? 'Сохранить' : 'Создать',
      onSubmit: async () => {
        titleF.setError('');
        const title = titleF.control.value.trim();
        if (!title) { titleF.setError('Укажите название'); return false; }
        if (!endings.length) { toast('Нужен хотя бы один финал', 'error'); return false; }
        for (const row of endings) {
          if (!row.titleF.control.value.trim()) {
            row.titleF.setError('Укажите название финала');
            return false;
          }
          row.titleF.setError('');
        }

        const payload = {
          id: src?.id || '',
          title,
          description: descF.control.value.trim(),
          sphere: sphereF.control.value.trim(),
          difficulty: diffF.control.value,
          player_role: playerRoleF.control.value.trim(),
          player_company: playerCompanyF.control.value.trim() || null,
          player_goal: playerGoalF.control.value.trim(),
          player_batna: playerBatnaF.control.value.trim(),
          partner_name: partnerNameF.control.value.trim(),
          partner_role: partnerRoleF.control.value.trim(),
          partner_company: partnerCompanyF.control.value.trim() || null,
          partner_goal: partnerGoalF.control.value.trim(),
          partner_goals: partnerGoalsF.control.value.split('\n').map((x) => x.trim()).filter(Boolean),
          partner_batna: partnerBatnaF.control.value.trim(),
          partner_personality: {
            tone: toneF.control.value.trim() || null,
            style: styleF.control.value.trim() || null,
            traits: traitsF.control.value.trim() || null,
          },
          opening_context: openingF.control.value.trim(),
          endings: endings.map((row) => ({
            id: row.id,
            title: row.titleF.control.value.trim(),
            text: row.textF.control.value.trim(),
            outcome: row.outcomeF.control.value.trim(),
            min_score: Number(row.minScoreF.control.value) || 0,
          })),
          ai_generated: src?.ai_generated || false,
          is_active: activeF.control.value === '1',
          created_by: src?.created_by ?? null,
          created_at: src?.created_at || '',
          updated_at: src?.updated_at ?? null,
        };

        if (isEdit) {
          await request(`/scenarios/${src.id}`, { method: 'PUT', body: payload });
          toast('Сценарий сохранён', 'success');
        } else {
          await request('/scenarios', { method: 'POST', body: payload });
          toast('Сценарий создан', 'success');
        }
        await load();
        return true;
      },
    });
  }

  // ── ИИ-генератор ──

  function openGenerator() {
    const briefF = field({
      label: 'Бриф', type: 'textarea', rows: 6, required: true,
      placeholder: 'Например: переговоры о скидке на годовую подписку с клиентом, который уходит к конкурентам…',
      hint: 'До 4000 символов. Опишите ситуацию, стороны и интересы.',
    });
    const diffF = field({
      label: 'Сложность', type: 'select', value: 'medium',
      options: DIFFICULTIES.map((d) => ({ value: d.value, label: d.label })),
    });
    const sphereF = field({ label: 'Сфера (необязательно)', placeholder: 'Продажи' });

    formModal({
      title: 'ИИ-генератор сценария',
      body: h('div.stack', null, briefF, h('div.input-row', null, diffF, sphereF)),
      submitLabel: 'Сгенерировать',
      onSubmit: async () => {
        briefF.setError('');
        const brief = briefF.control.value.trim();
        if (!brief) { briefF.setError('Опишите бриф для генерации'); return false; }
        if (brief.length > 4000) { briefF.setError('Бриф не длиннее 4000 символов'); return false; }
        const sphere = sphereF.control.value.trim();
        const created = await request('/scenarios/generate', {
          method: 'POST',
          body: {
            brief,
            difficulty: diffF.control.value,
            ...(sphere ? { sphere } : {}),
          },
        });
        toast(`Черновик «${created.title}» сгенерирован — не опубликован`, 'success');
        await load();
        // Открыть результат на редактирование/вычитку
        openEditor(created);
        return true;
      },
    });
  }

  load();
}
