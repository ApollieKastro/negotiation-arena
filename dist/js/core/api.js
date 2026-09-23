// HTTP-клиент API: /api/v1, Bearer, один retry через refresh при 401

import { accessToken, updateSession, clear } from './store.js';
import { navigate } from './router.js';

const BASE = '/api/v1';

/** Ошибка API: {status, message} — message из поля error */
export class ApiError extends Error {
  constructor(status, message) {
    super(message || `HTTP ${status}`);
    this.name = 'ApiError';
    this.status = status;
  }
}

/**
 * request(path, {method, body, query, auth}) → JSON
 * auth=true (по умолчанию) — Bearer; при 401 — одна попытка refresh.
 */
export async function request(path, { method = 'GET', body, query, auth = true, _isRetry = false } = {}) {
  const url = buildUrl(path, query);
  const headers = { Accept: 'application/json' };
  const token = accessToken();

  if (body !== undefined && body !== null && !(body instanceof FormData)) {
    headers['Content-Type'] = 'application/json';
  }
  if (auth && token) headers.Authorization = `Bearer ${token}`;

  let res;
  try {
    res = await fetch(url, {
      method,
      headers,
      body:
        body === undefined || body === null
          ? undefined
          : body instanceof FormData
            ? body
            : JSON.stringify(body),
    });
  } catch {
    throw new ApiError(0, 'Сервер недоступен — проверьте соединение');
  }

  // 401 → одна попытка refresh с текущим JWT
  if (res.status === 401 && auth && !_isRetry) {
    const refreshed = await tryRefresh();
    if (refreshed) {
      return request(path, { method, body, query, auth, _isRetry: true });
    }
    // refresh не удался → logout + редирект
    clear();
    navigate('#/login', { replace: true });
    throw new ApiError(401, 'Сессия истекла — войдите снова');
  }

  const data = await parseBody(res);

  if (!res.ok) {
    const message =
      (data && typeof data === 'object' && data.error) ||
      (typeof data === 'string' && data) ||
      defaultMessage(res.status);
    throw new ApiError(res.status, message);
  }

  return data;
}

/** POST /auth/refresh {token} → AuthSession | null */
async function tryRefresh() {
  const token = accessToken();
  if (!token) return false;
  try {
    const res = await fetch(`${BASE}/auth/refresh`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      body: JSON.stringify({ token }),
    });
    if (!res.ok) return false;
    const session = await res.json();
    if (!session || !session.token) return false;
    updateSession(session); // тот же JWT-контракт, без повторного push настроек
    return true;
  } catch {
    return false;
  }
}

function buildUrl(path, query) {
  const clean = String(path).replace(/^\//, '');
  let url = `${BASE}/${clean}`;
  if (query) {
    const params = new URLSearchParams();
    for (const [k, v] of Object.entries(query)) {
      if (v === undefined || v === null || v === '') continue;
      params.set(k, String(v));
    }
    const qs = params.toString();
    if (qs) url += `?${qs}`;
  }
  return url;
}

async function parseBody(res) {
  if (res.status === 204) return null;
  const text = await res.text();
  if (!text) return null;
  try { return JSON.parse(text); } catch { return text; }
}

function defaultMessage(status) {
  if (status === 401) return 'Требуется вход';
  if (status === 403) return 'Недостаточно прав';
  if (status === 404) return 'Не найдено';
  if (status >= 500) return 'Ошибка сервера';
  return `Ошибка запроса (${status})`;
}
