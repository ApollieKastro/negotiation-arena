// Страница диалога: лента сообщений, ходы, голос (STT/TTS), финиш/бросить

import { h, fmtDateTime } from '../core/dom.js';
import {
  emptyState, spinner, toast, badge, confirmModal, tabs,
} from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { navigate } from '../core/router.js';
import { accessToken } from '../core/store.js';
import {
  t, difficultyLabel, statusLabel, strategyLabel, spinLabel,
} from '../core/i18n.js';

const DIFF_VARIANT = { easy: 'success', medium: 'warning', hard: 'danger' };
const STRATEGY_VARIANT = {
  collaboration: 'success',
  compromise: 'warning',
  confrontation: 'danger',
};

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
  return h(`span.badge.no-dot.${cls}`, { text: t('session.scoreChip', { n: label }) });
}

function strategyBadge(slug) {
  if (!slug) return null;
  return badge(
    strategyLabel(slug) || slug,
    STRATEGY_VARIANT[slug] || 'neutral',
    { dot: false }
  );
}

function spinBadge(code) {
  if (!code) return null;
  return h('span.badge.badge-info.no-dot', {
    title: spinLabel(code) || code,
    text: `SPIN ${code}`,
  });
}

/** Оценка LLM-судьи: стратегия/аргументация/тон, 0–10 */
function judgeChip(j) {
  if (!j) return null;
  return h('span.badge.badge-info.no-dot', {
    title: t('session.judgeTitle', { s: j.strategy, a: j.argument, t: j.tone }),
    text: t('session.judgeChip', { n: `${j.strategy}/${j.argument}/${j.tone}` }),
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
  let judgeByMsgId = new Map();
  let busy = false;
  let voiceOn = false;
  let mediaRecorder = null;
  let recChunks = [];
  let recState = 'idle'; // idle | recording

  // ── Шапка ──
  const scoreEl = h('strong.num', { text: '—' });
  const turnsEl = h('span', { text: '—' });
  const statusEl = h('span', { text: '' });
  const detailToggle = h('button.btn.btn-ghost.btn-sm', { type: 'button', text: t('session.detailsClose') });

  const headerCard = h('div.card', null,
    h('div.card-body', null,
      h('div.row-between', null,
        h('div.stack', { style: { gap: '4px' } },
          h('div.row', null,
            h('strong', { id: 'session-title', text: t('session.title') }),
            statusEl
          ),
          h('div.small.muted', { text: fmtDateTime(new Date().toISOString()) })
        ),
        h('div.row', null,
          h('span.small.muted', null, t('session.turns')),
          turnsEl,
          h('span.small.muted', null, t('session.score')),
          scoreEl,
          detailToggle,
          h('button.btn.btn-danger.btn-sm', { type: 'button', id: 'btn-abandon', text: t('action.abandon') }),
          h('button.btn.btn-primary.btn-sm', { type: 'button', id: 'btn-finish', text: t('action.finish') }),
          h('button.btn.btn-secondary.btn-sm', { type: 'button', id: 'btn-result', text: t('action.result'), style: { display: 'none' } })
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
  }, spinner('sm'), h('span.small', { text: t('session.partnerThinking') }));

  const textarea = h('textarea.input', {
    rows: '2',
    placeholder: t('session.composerPlaceholder'),
    'aria-label': t('session.composerAria'),
    style: { resize: 'vertical', minHeight: '56px' },
  });

  const sendBtn = h('button.btn.btn-primary', { type: 'button', text: t('action.send') });
  const micBtn = h('button.btn.btn-secondary', { type: 'button', text: t('session.record'), style: { display: 'none' } });
  const voiceLabel = h('span.small.muted', { text: t('session.voiceOffLabel') });

  const voiceToggle = h('button.btn.btn-ghost.btn-sm', {
    type: 'button',
    'aria-pressed': 'false',
    text: t('session.voiceOff'),
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
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: t('session.title') }),
        h('div.page-sub', { text: t('session.sub') })
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
    detailToggle.textContent = detailsOpen ? t('session.detailsOpen') : t('session.detailsClose');
  });

  function renderDetails() {
    if (!scenario) {
      detailsHost.replaceChildren(h('div.small.muted', { text: t('session.scenarioNotLoaded') }));
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
        { id: 'me', label: t('session.tabMe') },
        { id: 'partner', label: t('session.tabPartner') },
        { id: 'company', label: t('session.tabCompany') },
      ],
      active: 'me',
    });

    const panes = {
      me: infoBlock(t('session.tabMe'), [
        [t('scenarios.role'), sc.player_role],
        [t('scenarios.company'), sc.player_company],
        [t('scenarios.goal'), sc.player_goal],
        [t('scenarios.batna'), sc.player_batna],
      ]),
      partner: infoBlock(t('scenarios.partner', { name: sc.partner_name }), [
        [t('scenarios.role'), sc.partner_role],
        [t('scenarios.company'), sc.partner_company],
        [t('scenarios.goal'), sc.partner_goal],
        [t('scenarios.personality'), personality],
      ]),
      company: infoBlock(t('session.context'), [
        [t('session.sphere'), sc.sphere],
        [t('session.difficulty'), difficultyLabel(sc.difficulty) || sc.difficulty],
        [t('session.yourCompany'), sc.player_company],
        [t('session.partnerCompany'), sc.partner_company],
      ]),
    };

    const paneHost = h('div.mt-3', null, panes.me);
    tabsEl.addEventListener('click', () => setTimeout(() => {
      paneHost.replaceChildren(panes[tabsEl.getActive()] || panes.me);
    }, 0));

    detailsHost.replaceChildren(
      h('div.row', null,
        h('span.small.muted', { text: sc.title }),
        badge(difficultyLabel(sc.difficulty) || sc.difficulty, DIFF_VARIANT[sc.difficulty] || 'neutral')
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
      const judge = judgeByMsgId.get(msg.id);
      if (judge) meta.push(judgeChip(judge));
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
        ? h('div.small.muted', { text: scenario ? scenario.partner_name : t('session.partner'), style: { marginBottom: '4px' } })
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
    titleEl.textContent = scenario ? scenario.title : t('session.title');
    scoreEl.textContent = String(session.total_score ?? 0);
    turnsEl.textContent = String(session.turn_count ?? 0);
    const st = session.status;
    statusEl.replaceChildren(badge(
      statusLabel(st),
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
      textarea.placeholder = t('session.finishedPlaceholder');
      micBtn.style.display = 'none';
    }
  }

  function setBusy(v) {
    busy = v;
    sendBtn.disabled = v || !session || session.status !== 'active';
    sendBtn.textContent = v ? t('action.sending') : t('action.send');
    textarea.disabled = v || !session || session.status !== 'active';
    thinkingEl.style.display = v ? '' : 'none';
    if (v) scrollFeed();
    applyStatusUI();
    if (!v && session && session.status === 'active') sendBtn.textContent = t('action.send');
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
      if (out.judge) judgeByMsgId.set(playerMsg.id, out.judge);

      appendMessage(playerMsg);
      appendMessage(partnerMsg);

      session = out.session;
      updateHeader();
      attachTtsButtons();

      const d = Number(out.player_score_delta) || 0;
      toast(
        d > 0 ? t('session.scorePlus', { n: d }) :
        d < 0 ? t('session.scoreMinus', { n: d }) :
        t('session.scoreZero'),
        d > 0 ? 'success' : d < 0 ? 'warning' : 'info'
      );
    } catch (err) {
      if (!isCurrent()) return;
      textarea.value = text;
      const msg = err instanceof ApiError ? err.message : t('session.sendFail');
      if (err instanceof ApiError && err.status === 400 &&
          (/лимит ходов/i.test(msg) || /сессия уже завершена/i.test(msg) || /заверш/i.test(msg))) {
        toast(msg, 'warning', 6000);
        const ok = await confirmModal({
          title: t('session.finishNowTitle'),
          message: `${msg}. ${t('session.finishMsg')}`,
          confirmText: t('action.finish'),
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
      title: t('session.speak'),
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
        let message = `TTS error (${res.status})`;
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
      toast(t('session.playFail'), 'error');
    }
  }

  // ── Голос: переключатель + запись + STT ──
  voiceToggle.addEventListener('click', () => {
    if (!session || session.status !== 'active') return;
    voiceOn = !voiceOn;
    voiceToggle.setAttribute('aria-pressed', voiceOn ? 'true' : 'false');
    voiceToggle.textContent = voiceOn ? t('session.voiceOn') : t('session.voiceOff');
    voiceLabel.textContent = voiceOn ? t('session.voiceOnLabel') : t('session.voiceOffLabel');
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
      toast(t('session.noMedia'), 'error');
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      recChunks = [];
      mediaRecorder = new MediaRecorder(stream, { mimeType: 'audio/webm' });
      mediaRecorder.ondataavailable = (e) => { if (e.data && e.data.size) recChunks.push(e.data); };
      mediaRecorder.onstop = async () => {
        stream.getTracks().forEach((tr) => tr.stop());
        recState = 'idle';
        micBtn.textContent = t('session.record');
        micBtn.classList.remove('btn-danger');
        micBtn.classList.add('btn-secondary');
        const blob = new Blob(recChunks, { type: 'audio/webm' });
        recChunks = [];
        if (!blob.size) { toast(t('session.recordEmpty'), 'warning'); return; }
        await transcribe(blob);
      };
      mediaRecorder.start();
      recState = 'recording';
      micBtn.textContent = t('session.stop');
      micBtn.classList.remove('btn-secondary');
      micBtn.classList.add('btn-danger');
      toast(t('session.recording'), 'info', 2500);
    } catch {
      toast(t('session.noMic'), 'error');
    }
  });

  async function transcribe(blob) {
    const fd = new FormData();
    fd.append('file', blob, 'recording.webm');
    micBtn.disabled = true;
    micBtn.textContent = t('session.recognizing');
    try {
      const res = await request('/voice/stt', { method: 'POST', body: fd });
      const text = (res && res.text ? String(res.text) : '').trim();
      if (!text) { toast(t('session.speechEmpty'), 'warning'); return; }
      textarea.value = text;
      voiceOn = false;
      voiceToggle.setAttribute('aria-pressed', 'false');
      voiceToggle.textContent = t('session.voiceOff');
      voiceLabel.textContent = t('session.voiceOffLabel');
      textRow.style.display = '';
      micBtn.style.display = 'none';
      toast(t('session.recognized'), 'success');
      textarea.focus();
    } catch (err) {
      const msg = err instanceof ApiError ? err.message : t('common.error');
      if (err instanceof ApiError && err.status === 503) toastVoice503(msg);
      else toast(msg, 'error');
    } finally {
      micBtn.disabled = false;
      micBtn.textContent = t('session.record');
      applyStatusUI();
    }
  }

  // ── Завершить / Бросить ──
  async function doFinish() {
    try {
      await request(`/sessions/${id}/finish`, { method: 'POST' });
      toast(t('session.finishOk'), 'success');
      navigate(`#/result/${id}`);
    } catch (err) {
      toast(err instanceof ApiError ? err.message : t('session.finishFail'), 'error');
    }
  }

  btnFinish.addEventListener('click', async () => {
    const ok = await confirmModal({
      title: t('session.finishTitle'),
      message: t('session.finishMsg'),
      confirmText: t('action.finish'),
    });
    if (ok) await doFinish();
  });

  btnAbandon.addEventListener('click', async () => {
    const ok = await confirmModal({
      title: t('session.abandonTitle'),
      message: t('session.abandonMsg'),
      confirmText: t('action.abandon'),
      danger: true,
    });
    if (!ok) return;
    try {
      await request(`/sessions/${id}/abandon`, { method: 'POST' });
      toast(t('session.abandonOk'), 'info');
      navigate('#/history');
    } catch (err) {
      toast(err instanceof ApiError ? err.message : t('session.abandonFail'), 'error');
    }
  });

  btnResult.addEventListener('click', () => navigate(`#/result/${id}`));

  // ── Загрузка ──
  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: t('session.title') }),
        h('div.page-sub', { text: t('session.loading') })
      )
    ),
    h('div.card', null, h('div.card-body', null, spinner('lg', { label: t('session.loadingSession') })))
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
          h('h1', { text: t('session.title') }),
          h('div.page-sub', { text: fmtDateTime(s.created_at) })
        )
      ),
      headerCard,
      h('div.mt-4', null, feed),
      composer
    );

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
          h('h1', { text: t('session.title') }),
          h('div.page-sub', { text: t('session.loadError') })
        )
      ),
      emptyState({
        icon: '⚠',
        title: t('session.openError'),
        description: err instanceof ApiError ? err.message : t('common.error'),
        action: h('button.btn.btn-secondary', {
          type: 'button',
          text: t('action.history'),
          onClick: () => navigate('#/history'),
        }),
      })
    );
  });
}
