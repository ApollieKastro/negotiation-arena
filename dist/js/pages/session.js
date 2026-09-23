// Страница диалога: лента сообщений, ходы, голос (STT/TTS), финиш/бросить

import { h, fmtDateTime } from '../core/dom.js';
import {
  emptyState, spinner, toast, badge, confirmModal, tabs,
} from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { navigate } from '../core/router.js';
import { accessToken } from '../core/store.js';

const DIFF_LABEL = { easy: 'Начальная', medium: 'Средняя', hard: 'Сложная' };
const DIFF_VARIANT = { easy: 'success', medium: 'warning', hard: 'danger' };
const STRATEGY_LABEL = {
  collaboration: 'Сотрудничество',
  compromise: 'Компромисс',
  confrontation: 'Конфронтация',
};
const STRATEGY_VARIANT = {
  collaboration: 'success',
  compromise: 'warning',
  confrontation: 'danger',
};
const SPIN_TITLES = { S: 'Ситуация', P: 'Проблема', I: 'Последствия', N: 'Ценность решения' };

const BUBBLE_BASE = {
  maxWidth: 'min(78%, 640px)',
  padding: '10px 14px',
  borderRadius: 'var(--radius-lg)',
  fontSize: 'var(--fs-base)',
  lineHeight: '1.5',
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
};

let voice503Toasted = false;

function toastVoice503(message) {
  if (voice503Toasted) return;
  voice503Toasted = true;
  toast(message, 'error', 6000);
}

function deltaChip(delta) {
  const n = Number(delta) || 0;
  const cls = n > 0 ? 'badge-success' : n < 0 ? 'badge-danger' : 'badge-neutral';
  const label = n > 0 ? `+${n}` : String(n);
  return h(`span.badge.no-dot.${cls}`, { text: `Балл ${label}` });
}

function strategyBadge(slug) {
  if (!slug) return null;
  return badge(
    STRATEGY_LABEL[slug] || slug,
    STRATEGY_VARIANT[slug] || 'neutral',
    { dot: false }
  );
}

function spinBadge(code) {
  if (!code) return null;
  return h('span.badge.badge-info.no-dot', {
    title: SPIN_TITLES[code] || code,
    text: `SPIN ${code}`,
  });
}

export function renderPage(root, params = {}) {
  const id = params.id;
  // Guard от гонки: страницу могли покинуть, пока летел запрос
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'session' });
  const isCurrent = () => marker.isConnected;
  let session = null;
  let scenario = null;
  let spinByMsgId = new Map();
  let busy = false;
  let voiceOn = false;
  let mediaRecorder = null;
  let recChunks = [];
  let recState = 'idle'; // idle | recording

  // ── Шапка ──
  const scoreEl = h('strong.num', { text: '—' });
  const turnsEl = h('span', { text: '—' });
  const statusEl = h('span', { text: '' });
  const detailToggle = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: 'Детали ▾' });

  const headerCard = h('div.card', null,
    h('div.card-body', null,
      h('div.row-between', null,
        h('div.stack', { style: { gap: '4px' } },
          h('div.row', null,
            h('strong', { id: 'session-title', text: 'Сессия' }),
            statusEl
          ),
          h('div.small.muted', { text: fmtDateTime(new Date().toISOString()) })
        ),
        h('div.row', null,
          h('span.small.muted', null, 'Ходов: '),
          turnsEl,
          h('span.small.muted', null, 'Балл: '),
          scoreEl,
          detailToggle,
          h('button.btn.btn-danger.btn-sm', { type: 'button', id: 'btn-abandon', text: 'Бросить' }),
          h('button.btn.btn-primary.btn-sm', { type: 'button', id: 'btn-finish', text: 'Завершить' }),
          h('button.btn.btn-secondary.btn-sm', { type: 'button', id: 'btn-result', text: 'Посмотреть результат', style: { display: 'none' } })
        )
      ),
      h('div.mt-3', { id: 'session-details', style: { display: 'none' } })
    )
  );

  const feed = h('div', {
    id: 'chat-feed',
    style: {
      display: 'flex',
      flexDirection: 'column',
      gap: 'var(--sp-3)',
      overflowY: 'auto',
      padding: 'var(--sp-4)',
      minHeight: '280px',
      maxHeight: 'min(58vh, 640px)',
      background: 'var(--surface-2)',
      border: '1px solid var(--border)',
      borderRadius: 'var(--radius-lg)',
    },
  });

  const thinking = h('div.row', {
    id: 'thinking',
    style: { display: 'none', color: 'var(--muted)' },
  }, spinner('sm'), h('span.small', { text: 'Собеседник думает…' }));

  const textarea = h('textarea.input', {
    rows: '2',
    placeholder: 'Ваша реплика… (Ctrl+Enter — отправить)',
    'aria-label': 'Текст реплики',
    style: { resize: 'vertical', minHeight: '56px' },
  });

  const sendBtn = h('button.btn.btn-primary', { type: 'button', text: 'Отправить' });
  const micBtn = h('button.btn.btn-secondary', { type: 'button', text: '🎙 Записать', style: { display: 'none' } });
  const voiceLabel = h('span.small.muted', { text: 'Голос: выкл.' });

  const voiceToggle = h('button.btn.btn-ghost.btn-sm', {
    type: 'button',
    'aria-pressed': 'false',
    text: 'Голос — выкл.',
  });

  const textRow = h('div.row', { style: { alignItems: 'flex-end', gap: 'var(--sp-2)' } },
    h('div', { style: { flex: '1', minWidth: '200px' } }, textarea),
    sendBtn
  );

  const composer = h('div.stack.mt-3', { style: { gap: 'var(--sp-2)' } },
    thinking,
    textRow,
    micBtn,
    h('div.row-between', null, voiceLabel, voiceToggle)
  );

  const fatalError = h('div');

  root.replaceChildren(
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: 'Сессия' }),
        h('div.page-sub', { text: 'Диалог с собеседником' })
      )
    ),
    fatalError,
    headerCard,
    h('div.mt-4', { style: { display: 'flex', flexDirection: 'column', gap: '0' } }, feed),
    composer
  );

  const detailsHost = headerCard.querySelector('#session-details');
  const btnFinish = headerCard.querySelector('#btn-finish');
  const btnAbandon = headerCard.querySelector('#btn-abandon');
  const btnResult = headerCard.querySelector('#btn-result');
  const titleEl = headerCard.querySelector('#session-title');
  const thinkingEl = composer.querySelector('#thinking');

  // ── Сворачивание деталей ──
  let detailsOpen = false;
  detailToggle.addEventListener('click', () => {
    detailsOpen = !detailsOpen;
    detailsHost.style.display = detailsOpen ? '' : 'none';
    detailToggle.textContent = detailsOpen ? 'Детали ▴' : 'Детали ▾';
  });

  function renderDetails() {
    if (!scenario) {
      detailsHost.replaceChildren(h('div.small.muted', { text: 'Сценарий не загружен.' }));
      return;
    }
    const sc = scenario;
    const p = sc.partner_personality || {};
    const personality = [p.tone && `тон: ${p.tone}`, p.style && `манера: ${p.style}`, p.traits && `черты: ${p.traits}`]
      .filter(Boolean).join(', ') || '—';

    const infoBlock = (title, rows) => h('div.card', null,
      h('div.card-body.stack', { style: { gap: 'var(--sp-2)' } },
        h('div.card-title', { text: title }),
        ...rows.map(([label, value]) => h('div.small', null,
          h('span.muted', { text: `${label}: ` }),
          h('span', { text: value || '—' })
        ))
      )
    );

    const tabsEl = tabs({
      items: [
        { id: 'me', label: 'Ты' },
        { id: 'partner', label: 'Собеседник' },
        { id: 'company', label: 'Компания' },
      ],
      active: 'me',
    });

    const panes = {
      me: infoBlock('Ты', [
        ['Роль', sc.player_role],
        ['Компания', sc.player_company],
        ['Цель', sc.player_goal],
        ['BATNA', sc.player_batna],
      ]),
      partner: infoBlock(`Собеседник: ${sc.partner_name}`, [
        ['Роль', sc.partner_role],
        ['Компания', sc.partner_company],
        ['Цель', sc.partner_goal],
        ['Личность', personality],
      ]),
      company: infoBlock('Контекст', [
        ['Сфера', sc.sphere],
        ['Сложность', DIFF_LABEL[sc.difficulty] || sc.difficulty],
        ['Твоя компания', sc.player_company],
        ['Его компания', sc.partner_company],
      ]),
    };

    const paneHost = h('div.mt-3', null, panes.me);
    tabsEl.addEventListener('click', () => setTimeout(() => {
      paneHost.replaceChildren(panes[tabsEl.getActive()] || panes.me);
    }, 0));

    detailsHost.replaceChildren(
      h('div.row', null,
        h('span.small.muted', { text: sc.title }),
        badge(DIFF_LABEL[sc.difficulty] || sc.difficulty, DIFF_VARIANT[sc.difficulty] || 'neutral')
      ),
      tabsEl,
      paneHost
    );
  }

  // ── Рендер сообщений ──
  function messageNode(msg) {
    const isPlayer = msg.role === 'player';
    const wrap = h('div', {
      dataset: { role: msg.role },
      style: {
        display: 'flex',
        justifyContent: isPlayer ? 'flex-end' : 'flex-start',
      },
    });

    const meta = [];
    if (isPlayer) {
      if (typeof msg.score_delta === 'number' && msg.score_delta !== 0) meta.push(deltaChip(msg.score_delta));
      const sb = strategyBadge(msg.strategy);
      if (sb) meta.push(sb);
      const spin = spinByMsgId.get(msg.id);
      if (spin) meta.push(spinBadge(spin));
    }

    const bubble = h('div', {
      style: {
        ...BUBBLE_BASE,
        background: isPlayer ? 'var(--accent-soft)' : 'var(--surface-3)',
        border: `1px solid ${isPlayer ? 'color-mix(in srgb, var(--accent) 35%, transparent)' : 'var(--border)'}`,
        borderLeft: isPlayer ? '4px solid var(--accent)' : '4px solid var(--border-strong)',
        textAlign: 'left',
      },
    },
      !isPlayer
        ? h('div.small.muted', { text: scenario ? scenario.partner_name : 'Собеседник', style: { marginBottom: '4px' } })
        : null,
      h('div', { text: msg.content }),
      meta.length
        ? h('div.row', { style: { gap: 'var(--sp-1)', marginTop: '6px' } }, ...meta)
        : null
    );

    wrap.append(bubble);
    return wrap;
  }

  function appendMessage(msg) {
    feed.append(messageNode(msg));
    feed.scrollTop = feed.scrollHeight;
  }

  function renderFeed(messages) {
    feed.replaceChildren(...messages.map(messageNode));
    feed.scrollTop = feed.scrollHeight;
  }

  function scrollFeed() {
    feed.scrollTop = feed.scrollHeight;
  }

  // ── Обновление шапки ──
  function updateHeader() {
    if (!session) return;
    titleEl.textContent = scenario ? scenario.title : 'Сессия';
    scoreEl.textContent = String(session.total_score ?? 0);
    turnsEl.textContent = String(session.turn_count ?? 0);
    const st = session.status;
    statusEl.replaceChildren(badge(
      st === 'active' ? 'Активна' : st === 'finished' ? 'Завершена' : 'Брошена',
      st === 'active' ? 'info' : st === 'finished' ? 'success' : 'neutral'
    ));
    applyStatusUI();
  }

  function applyStatusUI() {
    const active = session && session.status === 'active';
    textarea.disabled = !active || busy;
    sendBtn.disabled = !active || busy;
    micBtn.disabled = !active || busy || !voiceOn;
    btnFinish.style.display = active ? '' : 'none';
    btnAbandon.style.display = active ? '' : 'none';
    btnResult.style.display = active ? 'none' : '';
    if (!active) {
      textarea.placeholder = 'Сессия завершена — просмотрите результат.';
      micBtn.style.display = 'none';
    }
  }

  function setBusy(v) {
    busy = v;
    sendBtn.disabled = v || !session || session.status !== 'active';
    sendBtn.textContent = v ? 'Отправка…' : 'Отправить';
    textarea.disabled = v || !session || session.status !== 'active';
    thinkingEl.style.display = v ? '' : 'none';
    if (v) scrollFeed();
    applyStatusUI();
    if (!v && session && session.status === 'active') sendBtn.textContent = 'Отправить';
  }

  // ── Ход ──
  async function submitTurn() {
    const text = textarea.value.trim();
    if (!text || busy || !session || session.status !== 'active') return;

    setBusy(true);
    textarea.value = '';
    try {
      const out = await request(`/sessions/${id}/turn`, {
        method: 'POST',
        body: { text },
      });
      if (!isCurrent()) return;

      const playerMsg = {
        id: `local-p-${Date.now()}`,
        role: 'player',
        content: text,
        strategy: out.strategy_slug,
        score_delta: out.player_score_delta,
        turn_index: session.turn_count,
      };
      const partnerMsg = {
        id: `local-o-${Date.now()}`,
        role: 'partner',
        content: out.partner_reply,
        strategy: null,
        score_delta: 0,
        turn_index: session.turn_count + 1,
      };
      if (out.spin_code) spinByMsgId.set(playerMsg.id, out.spin_code);

      appendMessage(playerMsg);
      appendMessage(partnerMsg);

      session = out.session;
      updateHeader();
      attachTtsButtons();

      const d = Number(out.player_score_delta) || 0;
      toast(
        d > 0 ? `Отличная реплика! +${d} к баллу` :
        d < 0 ? `Слабая реплика: ${d} к баллу` :
        'Ход засчитан, балл без изменений',
        d > 0 ? 'success' : d < 0 ? 'warning' : 'info'
      );
    } catch (err) {
      if (!isCurrent()) return;
      textarea.value = text;
      const msg = err instanceof ApiError ? err.message : 'Не удалось отправить ход';
      if (err instanceof ApiError && err.status === 400 &&
          (/лимит ходов/i.test(msg) || /сессия уже завершена/i.test(msg) || /заверш/i.test(msg))) {
        toast(msg, 'warning', 6000);
        const ok = await confirmModal({
          title: 'Завершить сессию?',
          message: `${msg}. Завершить сейчас и посмотреть отчёт?`,
          confirmText: 'Завершить',
        });
        if (ok) await doFinish();
      } else {
        toast(msg, 'error');
      }
    } finally {
      setBusy(false);
    }
  }

  sendBtn.addEventListener('click', submitTurn);
  textarea.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      submitTurn();
    }
  });

  // ── TTS-кнопка у последнего сообщения партнёра ──
  function attachTtsButtons() {
    feed.querySelectorAll('[data-tts]').forEach((el) => el.remove());
    const partnerWraps = [...feed.children].filter((el) => el.dataset && el.dataset.role === 'partner');
    if (!partnerWraps.length) return;
    const wrap = partnerWraps[partnerWraps.length - 1];
    const bubble = wrap.firstElementChild;
    if (!bubble) return;
    const textDivs = [...bubble.children].filter((c) => c.tagName === 'DIV');
    const contentDiv = textDivs[textDivs.length - 1];
    const text = contentDiv ? contentDiv.textContent : bubble.textContent;
    const btn = h('button.icon-btn', {
      type: 'button',
      'data-tts': '1',
      title: 'Озвучить реплику',
      text: '🔊',
      style: { fontSize: '14px', verticalAlign: 'middle' },
      onClick: () => playTts(text),
    });
    bubble.append(btn);
  }

  async function playTts(text) {
    if (!text) return;
    try {
      const token = accessToken();
      const res = await fetch('/api/v1/voice/tts', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Accept: 'audio/*',
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        body: JSON.stringify({ text }),
      });
      if (!res.ok) {
        let message = `Ошибка TTS (${res.status})`;
        try {
          const data = await res.json();
          if (data && data.error) message = data.error;
        } catch { /* non-JSON */ }
        if (res.status === 503) toastVoice503(message);
        else toast(message, 'error');
        return;
      }
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const audio = new Audio(url);
      audio.onended = () => URL.revokeObjectURL(url);
      await audio.play();
    } catch {
      toast('Не удалось воспроизвести голос', 'error');
    }
  }

  // ── Голос: переключатель + запись + STT ──
  voiceToggle.addEventListener('click', () => {
    if (!session || session.status !== 'active') return;
    voiceOn = !voiceOn;
    voiceToggle.setAttribute('aria-pressed', voiceOn ? 'true' : 'false');
    voiceToggle.textContent = voiceOn ? 'Голос — вкл.' : 'Голос — выкл.';
    voiceLabel.textContent = voiceOn
      ? 'Голос: включён — говорите вместо текста'
      : 'Голос: выкл.';
    textRow.style.display = voiceOn ? 'none' : '';
    micBtn.style.display = voiceOn ? '' : 'none';
    applyStatusUI();
  });

  micBtn.addEventListener('click', async () => {
    if (recState === 'recording' && mediaRecorder) {
      mediaRecorder.stop();
      return;
    }
    if (!navigator.mediaDevices || !window.MediaRecorder) {
      toast('Запись аудио не поддерживается браузером', 'error');
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      recChunks = [];
      mediaRecorder = new MediaRecorder(stream, { mimeType: 'audio/webm' });
      mediaRecorder.ondataavailable = (e) => { if (e.data && e.data.size) recChunks.push(e.data); };
      mediaRecorder.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop());
        recState = 'idle';
        micBtn.textContent = '🎙 Записать';
        micBtn.classList.remove('btn-danger');
        micBtn.classList.add('btn-secondary');
        const blob = new Blob(recChunks, { type: 'audio/webm' });
        recChunks = [];
        if (!blob.size) { toast('Запись пуста — попробуйте ещё', 'warning'); return; }
        await transcribe(blob);
      };
      mediaRecorder.start();
      recState = 'recording';
      micBtn.textContent = '⏹ Остановить';
      micBtn.classList.remove('btn-secondary');
      micBtn.classList.add('btn-danger');
      toast('Идёт запись — нажмите, чтобы остановить', 'info', 2500);
    } catch {
      toast('Нет доступа к микрофону', 'error');
    }
  });

  async function transcribe(blob) {
    const fd = new FormData();
    fd.append('file', blob, 'recording.webm');
    micBtn.disabled = true;
    micBtn.textContent = 'Распознаём…';
    try {
      const res = await request('/voice/stt', { method: 'POST', body: fd });
      const text = (res && res.text ? String(res.text) : '').trim();
      if (!text) { toast('Речь не распознана', 'warning'); return; }
      textarea.value = text;
      voiceOn = false;
      voiceToggle.setAttribute('aria-pressed', 'false');
      voiceToggle.textContent = 'Голос — выкл.';
      voiceLabel.textContent = 'Голос: выкл.';
      textRow.style.display = '';
      micBtn.style.display = 'none';
      toast('Распознано — проверьте текст и отправьте', 'success');
      textarea.focus();
    } catch (err) {
      const msg = err instanceof ApiError ? err.message : 'Ошибка распознавания';
      if (err instanceof ApiError && err.status === 503) toastVoice503(msg);
      else toast(msg, 'error');
    } finally {
      micBtn.disabled = false;
      micBtn.textContent = '🎙 Записать';
      applyStatusUI();
    }
  }

  // ── Завершить / Бросить ──
  async function doFinish() {
    try {
      await request(`/sessions/${id}/finish`, { method: 'POST' });
      toast('Сессия завершена!', 'success');
      navigate(`#/result/${id}`);
    } catch (err) {
      toast(err instanceof ApiError ? err.message : 'Не удалось завершить', 'error');
    }
  }

  btnFinish.addEventListener('click', async () => {
    const ok = await confirmModal({
      title: 'Завершить сессию?',
      message: 'Диалог закончится, будет построен отчёт с оценкой и рекомендациями.',
      confirmText: 'Завершить',
    });
    if (ok) await doFinish();
  });

  btnAbandon.addEventListener('click', async () => {
    const ok = await confirmModal({
      title: 'Бросить сессию?',
      message: 'Прогресс не сохранится в отчёт. Сессия будет помечена как брошенная.',
      confirmText: 'Бросить',
      danger: true,
    });
    if (!ok) return;
    try {
      await request(`/sessions/${id}/abandon`, { method: 'POST' });
      toast('Сессия прервана', 'info');
      navigate('#/history');
    } catch (err) {
      toast(err instanceof ApiError ? err.message : 'Не удалось прервать сессию', 'error');
    }
  });

  btnResult.addEventListener('click', () => navigate(`#/result/${id}`));

  // ── Загрузка ──
  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: 'Сессия' }),
        h('div.page-sub', { text: 'Загружаем…' })
      )
    ),
    h('div.card', null, h('div.card-body', null, spinner('lg', { label: 'Загружаем сессию…' })))
  );

  Promise.all([
    request(`/sessions/${id}`),
    request(`/sessions/${id}/messages`),
    request('/scenarios').catch(() => []),
  ]).then(([s, messages, scenarios]) => {
    if (!isCurrent()) return;
    session = s;
    scenario = (scenarios || []).find((x) => x.id === s.scenario_id) || null;
    if (!scenario) {
      // fallback: одиночный GET
      request(`/scenarios/${s.scenario_id}`).then((sc) => {
        if (!isCurrent()) return;
        scenario = sc;
        renderDetails();
      }).catch(() => {});
    }

    // Пересобираем нормальную разметку (убираем заглушку загрузки)
    root.replaceChildren(
      marker,
      h('div.page-header', null,
        h('div', null,
          h('h1', { text: 'Сессия' }),
          h('div.page-sub', { text: fmtDateTime(s.created_at) })
        )
      ),
      headerCard,
      h('div.mt-4', null, feed),
      composer
    );

    // Переподключаем обработчики, т.к. элементы те же (headerCard/feed/composer сохранены)
    renderDetails();
    updateHeader();
    renderFeed(messages || []);
    attachTtsButtons();
    textarea.focus();
  }).catch((err) => {
    if (!isCurrent()) return;
    root.replaceChildren(
      marker,
      h('div.page-header', null,
        h('div', null,
          h('h1', { text: 'Сессия' }),
          h('div.page-sub', { text: 'Ошибка загрузки' })
        )
      ),
      emptyState({
        icon: '⚠',
        title: 'Не удалось открыть сессию',
        description: err instanceof ApiError ? err.message : 'Ошибка запроса',
        action: h('button.btn.btn-secondary', {
          type: 'button',
          text: 'В историю',
          onClick: () => navigate('#/history'),
        }),
      })
    );
  });
}
