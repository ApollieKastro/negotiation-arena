// Телефонный режим: push-to-talk звонок с собеседником.
// Нажал и удерживал кнопку — говоришь; отпустил — STT → ход → ответ голосом.

import { h, fmtDateTime, fmtDuration } from '../core/dom.js';
import {
  emptyState, spinner, toast, badge, confirmModal,
} from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { navigate } from '../core/router.js';
import { accessToken } from '../core/store.js';
import { messageNode, createChatFeed } from '../core/chat.js';
import { t, statusLabel } from '../core/i18n.js';

/** Минимальная длительность записи (мс) — короткие касания не отправляем. */
const MIN_REC_MS = 400;

/** Состояния звонка. */
const State = {
  SETUP: 'setup',           // экран «Начать звонок» (user-gesture для autoplay)
  IDLE: 'idle',             // готов, ждём нажатия на микрофон
  WARMING: 'warming',       // добываем микрофон (первый раз / fallback)
  RECORDING: 'recording',   // говорим (удерживаем кнопку)
  TRANSCRIBING: 'transcribing',
  THINKING: 'thinking',     // ждём ответ LLM
  SPEAKING: 'speaking',     // воспроизводим TTS
  ENDED: 'ended',
};

function header(title, sub) {
  return h('div.page-header', null,
    h('div', null,
      h('h1', { text: title }),
      h('div.page-sub', { text: sub })
    )
  );
}

export function renderPage(root, params = {}) {
  const id = params.id;
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'call' });
  const isCurrent = () => marker.isConnected;

  let session = null;
  let scenario = null;
  let state = State.SETUP;
  let callStartedAt = 0;
  let timerHandle = null;

  // Запись (push-to-talk); stream прогревается при старте звонка
  let mediaStream = null;
  let recorder = null;
  let recChunks = [];
  let pressAt = 0;            // момент нажатия (для MIN_REC_MS)
  let recStopTimer = null;    // отложенный stop до мин. длительности
  let releasedDuringWarm = false; // отпустил, пока ещё добывали микрофон

  // Воспроизведение
  let currentAudio = null;

  const partnerName = () => (scenario ? scenario.partner_name : t('session.partner'));

  // ── Элементы ──
  const statusText = h('div.call-status', { text: t('call.setupHint') });
  const timerEl = h('span.call-timer.num', { text: '00:00' });
  const scoreEl = h('strong.num', { text: '—' });
  const turnsEl = h('strong.num', { text: '—' });

  const avatarEl = h('div.call-partner', null,
    h('span.call-partner-initials', { text: '?' })
  );

  const pttBtn = h('button.ptt-btn', {
    type: 'button',
    'aria-label': t('call.pttAria'),
    disabled: true,
  }, h('span.ptt-icon', { text: '🎙' }));

  const hangupBtn = h('button.btn.btn-danger', { type: 'button', text: t('call.hangup') });
  const toChatBtn = h('button.btn.btn-secondary', { type: 'button', text: t('call.toChat') });
  const replayBtn = h('button.btn.btn-ghost', { type: 'button', text: t('call.replay') });

  const transcript = createChatFeed({ id: 'call-transcript' });
  transcript.style.maxHeight = 'min(40vh, 420px)';

  const stage = h('div.call-stage', null,
    avatarEl,
    statusText,
    h('div.row', { style: { gap: 'var(--sp-4)', alignItems: 'center', justifyContent: 'center' } },
      h('span.small.muted', null, t('session.turns'), turnsEl),
      h('span.small.muted', null, t('session.score'), scoreEl),
      timerEl
    ),
    pttBtn,
    h('div.small.muted', { text: t('call.pttHint') }),
    h('div.call-controls', null, replayBtn, toChatBtn, hangupBtn)
  );

  function cleanupAudio() {
    if (currentAudio) {
      currentAudio.pause();
      currentAudio.src = '';
      currentAudio = null;
    }
  }

  function stopRecordingTracks() {
    if (recStopTimer) { clearTimeout(recStopTimer); recStopTimer = null; }
    if (mediaStream) {
      mediaStream.getTracks().forEach((tr) => tr.stop());
      mediaStream = null;
    }
    recorder = null;
    recChunks = [];
  }

  function cleanupAll() {
    if (timerHandle) { clearInterval(timerHandle); timerHandle = null; }
    cleanupAudio();
    stopRecordingTracks();
  }

  // Guard: страницу покинули — глушим ресурсы
  const watchDog = setInterval(() => {
    if (!isCurrent()) { cleanupAll(); clearInterval(watchDog); }
  }, 1000);

  function setState(next) {
    state = next;
    if (!isCurrent()) return;

    const canTalk = (next === State.IDLE || next === State.WARMING)
      && session && session.status === 'active';
    pttBtn.disabled = !canTalk;
    pttBtn.classList.toggle('is-recording', next === State.RECORDING);
    hangupBtn.disabled = ![State.IDLE, State.RECORDING, State.WARMING, State.THINKING,
      State.TRANSCRIBING, State.SPEAKING].includes(next)
      || !session || session.status !== 'active';

    const labels = {
      [State.SETUP]: t('call.setupHint'),
      [State.IDLE]: t('call.ready'),
      [State.WARMING]: t('call.warming'),
      [State.RECORDING]: t('call.listening'),
      [State.TRANSCRIBING]: t('call.recognizing'),
      [State.THINKING]: t('call.thinking'),
      [State.SPEAKING]: t('call.speaking'),
      [State.ENDED]: t('call.ended'),
    };
    statusText.textContent = labels[next] || '';
    avatarEl.classList.toggle('is-speaking', next === State.SPEAKING);
    avatarEl.classList.toggle('is-listening', next === State.RECORDING || next === State.WARMING);
  }

  function updateMeta() {
    if (!session) return;
    scoreEl.textContent = String(session.total_score ?? 0);
    turnsEl.textContent = String(session.turn_count ?? 0);
  }

  function appendMsg(msg) {
    transcript.append(messageNode(msg, { partnerName: partnerName() }));
    transcript.scrollTop = transcript.scrollHeight;
  }

  function tickTimer() {
    if (!callStartedAt) return;
    timerEl.textContent = fmtDuration(Math.floor((Date.now() - callStartedAt) / 1000));
  }

  // ── TTS ──
  function playTts(text) {
    return new Promise((resolve) => {
      if (!text) { resolve(); return; }
      fetch('/api/v1/voice/tts', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Accept: 'audio/*',
          ...(accessToken() ? { Authorization: `Bearer ${accessToken()}` } : {}),
        },
        body: JSON.stringify({ text }),
      }).then(async (res) => {
        if (!res.ok) {
          let message = `TTS error (${res.status})`;
          try {
            const data = await res.json();
            if (data && data.error) message = data.error;
          } catch { /* non-JSON */ }
          toast(message, 'error');
          resolve();
          return;
        }
        const blob = await res.blob();
        const url = URL.createObjectURL(blob);
        const audio = new Audio(url);
        currentAudio = audio;
        audio.onended = () => { URL.revokeObjectURL(url); currentAudio = null; resolve(); };
        audio.onerror = () => { URL.revokeObjectURL(url); currentAudio = null; resolve(); };
        try {
          await audio.play();
        } catch {
          URL.revokeObjectURL(url);
          currentAudio = null;
          toast(t('session.playFail'), 'error');
          resolve();
        }
      }).catch(() => {
        toast(t('session.playFail'), 'error');
        resolve();
      });
    });
  }

  async function speak(text) {
    setState(State.SPEAKING);
    await playTts(text);
    if (isCurrent() && session && session.status === 'active') setState(State.IDLE);
    else if (isCurrent()) setState(State.ENDED);
  }

  // ── Push-to-talk: запись ──
  const AUDIO_CONSTRAINTS = {
    audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
  };

  /** Прогрев микрофона: getUserMedia до первого нажатия, чтобы PTT стартовал мгновенно. */
  async function ensureMic() {
    if (mediaStream && mediaStream.active) return mediaStream;
    if (!navigator.mediaDevices || !window.MediaRecorder) {
      throw new Error('no-media');
    }
    try {
      mediaStream = await navigator.mediaDevices.getUserMedia(AUDIO_CONSTRAINTS);
    } catch {
      mediaStream = null;
      throw new Error('no-mic');
    }
    return mediaStream;
  }

  function pickMime() {
    if (window.MediaRecorder.isTypeSupported('audio/webm')) return 'audio/webm';
    if (window.MediaRecorder.isTypeSupported('audio/mp4')) return 'audio/mp4';
    return '';
  }

  function beginRecorder() {
    const mime = pickMime();
    recorder = new MediaRecorder(mediaStream, mime ? { mimeType: mime } : undefined);
    recorder.ondataavailable = (e) => { if (e.data && e.data.size) recChunks.push(e.data); };
    recorder.onstop = onRecorderStop;
    recChunks = [];
    recorder.start(250); // timeslice: данные идут и до stop
    setState(State.RECORDING);
    if (releasedDuringWarm) {
      releasedDuringWarm = false;
      finishRecording();
    }
  }

  function startRecording() {
    if (state !== State.IDLE || !session || session.status !== 'active') return;
    pressAt = Date.now();
    releasedDuringWarm = false;

    if (mediaStream && mediaStream.active) {
      // Тёплый поток — запись стартует синхронно, гонки нет
      try {
        beginRecorder();
      } catch {
        toast(t('session.noMedia'), 'error');
      }
      return;
    }

    // Fallback: микрофон ещё не готов (прогрев не удался)
    setState(State.WARMING);
    ensureMic().then(() => {
      if (!isCurrent()) { stopRecordingTracks(); return; }
      if (state !== State.WARMING) return; // состояние сменили, пока добывали mic
      try {
        beginRecorder();
      } catch {
        setState(State.IDLE);
        toast(t('session.noMedia'), 'error');
      }
    }).catch((e) => {
      if (!isCurrent()) return;
      setState(State.IDLE);
      toast(e && e.message === 'no-media' ? t('session.noMedia') : t('session.noMic'), 'error');
    });
  }

  function finishRecording() {
    if (state === State.WARMING) {
      // Отпустил до готовности микрофона — остановим сразу после старта
      releasedDuringWarm = true;
      return;
    }
    if (state !== State.RECORDING || !recorder || recorder.state !== 'recording') return;
    if (recStopTimer) return; // stop уже запланирован
    const wait = Math.max(0, MIN_REC_MS - (Date.now() - pressAt));
    recStopTimer = setTimeout(() => {
      recStopTimer = null;
      if (recorder && recorder.state === 'recording') recorder.stop();
    }, wait);
  }

  async function onRecorderStop() {
    // Поток НЕ глушим — остаёмся «тёплыми» до конца звонка
    const type = (recChunks[0] && recChunks[0].type) || 'audio/webm';
    const blob = new Blob(recChunks, { type });
    recChunks = [];
    recorder = null;
    if (!isCurrent()) return;
    if (!blob.size) {
      setState(State.IDLE);
      toast(t('session.recordEmpty'), 'warning');
      return;
    }
    await processUtterance(blob);
  }

  async function processUtterance(blob) {
    setState(State.TRANSCRIBING);
    let text = '';
    try {
      const fd = new FormData();
      const ext = type => (type.includes('mp4') ? 'm4a' : 'webm');
      fd.append('file', blob, `utterance.${ext(blob.type)}`);
      const res = await request('/voice/stt', { method: 'POST', body: fd });
      text = (res && res.text ? String(res.text) : '').trim();
    } catch (err) {
      if (!isCurrent()) return;
      setState(State.IDLE);
      toast(err instanceof ApiError ? err.message : t('common.error'), 'error');
      return;
    }
    if (!isCurrent()) return;
    if (!text) {
      setState(State.IDLE);
      toast(t('session.speechEmpty'), 'warning');
      return;
    }
    await doTurn(text);
  }

  async function doTurn(text) {
    setState(State.THINKING);
    // Оптимистично показываем реплику игрока
    const localId = `local-p-${Date.now()}`;
    appendMsg({ id: localId, role: 'player', content: text, strategy: null, score_delta: 0 });

    try {
      const out = await request(`/sessions/${id}/turn`, { method: 'POST', body: { text } });
      if (!isCurrent()) return;
      session = out.session;
      updateMeta();

      // Обновляем чипы у последнего player-узла
      const lastPlayer = [...transcript.children]
        .reverse()
        .find((el) => el.dataset && el.dataset.role === 'player');
      if (lastPlayer) {
        const fresh = messageNode({
          id: localId,
          role: 'player',
          content: text,
          strategy: out.strategy_slug,
          score_delta: out.player_score_delta,
        }, { partnerName: partnerName() });
        lastPlayer.replaceWith(fresh);
      }

      appendMsg({
        id: `local-o-${Date.now()}`,
        role: 'partner',
        content: out.partner_reply,
        strategy: null,
        score_delta: 0,
      });

      if (session.status !== 'active') {
        await speak(out.partner_reply);
        if (isCurrent()) endCallUi();
        return;
      }
      await speak(out.partner_reply);
    } catch (err) {
      if (!isCurrent()) return;
      setState(State.IDLE);
      const msg = err instanceof ApiError ? err.message : t('session.sendFail');
      if (err instanceof ApiError && err.status === 400 && /лимит ходов|заверш/i.test(msg)) {
        toast(msg, 'warning', 6000);
      } else {
        toast(msg, 'error');
      }
    }
  }

  // ── Жесты PTT ──
  pttBtn.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    if (pttBtn.disabled) return;
    pttBtn.setPointerCapture?.(e.pointerId);
    startRecording();
  });
  pttBtn.addEventListener('pointerup', () => finishRecording());
  pttBtn.addEventListener('pointercancel', () => finishRecording());
  pttBtn.addEventListener('pointerleave', (e) => {
    // Кнопка захвачена capture — leave не должен рвать запись раньше отпускания;
    // но если capture недоступен, отпускаем.
    if (!pttBtn.hasPointerCapture?.(e.pointerId)) finishRecording();
  });
  pttBtn.addEventListener('contextmenu', (e) => e.preventDefault());

  // Space как PTT (когда не в фокусе на инпуте)
  function onKeydown(e) {
    if (e.code !== 'Space' || e.repeat) return;
    const tag = (document.activeElement && document.activeElement.tagName) || '';
    if (['INPUT', 'TEXTAREA', 'BUTTON'].includes(tag)) return;
    if (state !== State.IDLE && state !== State.WARMING) return;
    e.preventDefault();
    startRecording();
  }
  function onKeyup(e) {
    if (e.code !== 'Space') return;
    if (state === State.RECORDING || state === State.WARMING) {
      e.preventDefault();
      finishRecording();
    }
  }
  document.addEventListener('keydown', onKeydown);
  document.addEventListener('keyup', onKeyup);

  // ── Кнопки ──
  replayBtn.addEventListener('click', async () => {
    if (!transcript.children.length) return;
    const partnerWraps = [...transcript.children]
      .filter((el) => el.dataset && el.dataset.role === 'partner');
    if (!partnerWraps.length) return;
    const bubble = partnerWraps[partnerWraps.length - 1].firstElementChild;
    const textDivs = bubble ? [...bubble.children].filter((c) => c.tagName === 'DIV') : [];
    const contentDiv = textDivs[textDivs.length - 1];
    const text = contentDiv ? contentDiv.textContent : (bubble ? bubble.textContent : '');
    if (state === State.IDLE && text) await speak(text);
  });

  toChatBtn.addEventListener('click', () => {
    cleanupAll();
    navigate(`#/session/${id}`);
  });

  hangupBtn.addEventListener('click', async () => {
    if (!session || session.status !== 'active') return;
    const ok = await confirmModal({
      title: t('call.hangupTitle'),
      message: t('call.hangupMsg'),
      confirmText: t('call.hangup'),
      danger: true,
    });
    if (!ok || !isCurrent()) return;
    hangupBtn.disabled = true;
    try {
      await request(`/sessions/${id}/finish`, { method: 'POST' });
      toast(t('session.finishOk'), 'success');
      if (isCurrent()) endCallUi();
    } catch (err) {
      toast(err instanceof ApiError ? err.message : t('session.finishFail'), 'error');
      if (isCurrent()) hangupBtn.disabled = false;
    }
  });

  function endCallUi() {
    cleanupAll();
    setState(State.ENDED);
    if (!isCurrent()) return;
    stage.replaceChildren(
      h('div.call-partner.is-ended', null, h('span.call-partner-initials', { text: '☎' })),
      h('div.call-status', { text: t('call.ended') }),
      h('div.small.muted', { text: t('call.endedSub') }),
      h('div.call-controls', null,
        h('a.btn.btn-primary', { href: `#/result/${id}` }, t('action.result')),
        h('a.btn.btn-secondary', { href: `#/dialog/${id}` }, t('action.dialog')),
        h('a.btn.btn-ghost', { href: '#/history' }, t('action.history'))
      )
    );
  }

  // ── Загрузка ──
  root.replaceChildren(
    marker,
    header(t('call.title'), t('call.loading')),
    h('div.card', null, h('div.card-body', null, spinner('lg', { label: t('call.loading') })))
  );

  Promise.all([
    request(`/sessions/${id}`),
    request(`/sessions/${id}/messages`),
    request('/scenarios').catch(() => []),
  ]).then(async ([s, messages, scenarios]) => {
    if (!isCurrent()) { cleanupAll(); return; }
    session = s;
    scenario = (scenarios || []).find((x) => x.id === s.scenario_id) || null;

    // Инициалы партнёра
    const name = partnerName();
    avatarEl.replaceChildren(
      h('span.call-partner-initials', { text: (name || '?').trim().charAt(0).toUpperCase() })
    );

    const sub = scenario
      ? `${scenario.title} · ${fmtDateTime(s.created_at)}`
      : fmtDateTime(s.created_at);

    root.replaceChildren(
      marker,
      header(t('call.title'), sub),
      h('div.card', null, h('div.card-body', null,
        h('div.row-between.mb-3', null,
          h('div.row', null,
            badge(statusLabel(s.status), s.status === 'active' ? 'success' : 'neutral'),
            s.ending_title ? badge(s.ending_title, 'accent', { dot: false }) : null
          ),
          h('a.btn.btn-ghost.btn-sm', { href: `#/session/${id}` }, t('call.toChat'))
        ),
        stage
      )),
      h('div.mt-4', null,
        h('div.card-title', { text: t('call.transcript'), style: { marginBottom: 'var(--sp-2)' } }),
        transcript
      )
    );
    updateMeta();

    // Реплики
    const list = Array.isArray(messages) ? messages : [];
    if (list.length) {
      transcript.replaceChildren(...list.map((m) => messageNode(m, { partnerName: name })));
      transcript.scrollTop = transcript.scrollHeight;
    }

    if (s.status !== 'active') {
      setState(State.ENDED);
      endCallUi();
      return;
    }

    setState(State.SETUP);
    // Замена setup-экрана на кнопку «Начать»
    const startBtn = h('button.btn.btn-primary.btn-lg', {
      type: 'button',
      text: t('call.start'),
      style: { fontSize: 'var(--fs-lg)', padding: '12px 32px' },
      onClick: async () => {
        if (!isCurrent()) return;
        callStartedAt = Date.now();
        tickTimer();
        timerHandle = setInterval(tickTimer, 1000);
        // Прогрев микрофона до первого PTT — нажатие стартует мгновенно
        try {
          await ensureMic();
        } catch (e) {
          if (!isCurrent()) return;
          toast(e && e.message === 'no-media' ? t('session.noMedia') : t('session.noMic'), 'error');
        }
        if (!isCurrent()) return;
        setState(State.IDLE);
        // Приветствие собеседника — opening-реплика голосом (первый жест уже был)
        const opening = list.find((m) => m.role === 'partner' && m.turn_index === 0);
        if (opening && s.turn_count === 0) {
          await speak(opening.content);
        }
      },
    });
    stage.insertBefore(
      h('div.stack', { style: { alignItems: 'center', gap: 'var(--sp-2)' } },
        startBtn,
        h('div.small.muted', { text: t('call.startHint') })
      ),
      pttBtn // до PTT-кнопки
    );
    pttBtn.style.display = 'none';
    startBtn.addEventListener('click', () => {
      pttBtn.style.display = '';
      startBtn.closest('.stack')?.remove();
    }, { once: true });
  }).catch((err) => {
    if (!isCurrent()) { cleanupAll(); return; }
    root.replaceChildren(
      marker,
      header(t('call.title'), t('call.loadError')),
      emptyState({
        icon: '⚠',
        title: t('call.openError'),
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
