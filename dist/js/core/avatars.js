// Кэш аватаров: GET /users/:id/avatar (Bearer) → objectURL.
//
// <img> не умеет шлать Authorization — компонент avatar() грузит байты через
// fetch и подставляет blob-URL. Кэш живёт до перезагрузки страницы; после
// загрузки/удаления аватара вызывайте invalidateAvatar(userId).

import { accessToken } from './store.js';

const BASE = '/api/v1';

/** userId → objectURL */
const cache = new Map();

/**
 * objectURL аватара или null (нет аватара / ошибка / нет токена).
 * Ошибки молча глотаем — UI остаётся на инициалах.
 */
export async function fetchAvatarUrl(userId) {
  if (!userId) return null;
  if (cache.has(userId)) return cache.get(userId);
  const token = accessToken();
  if (!token) return null;

  let url = null;
  try {
    const res = await fetch(`${BASE}/users/${encodeURIComponent(userId)}/avatar`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (res.ok) {
      const blob = await res.blob();
      if (blob.size > 0) {
        url = URL.createObjectURL(blob);
        cache.set(userId, url);
      }
    }
  } catch { /* офлайн/сбой — инициалы */ }
  return url;
}

/** Сбрасывает кэш пользователя (после загрузки/удаления аватара). */
export function invalidateAvatar(userId) {
  if (!userId) return;
  const url = cache.get(userId);
  if (url) {
    URL.revokeObjectURL(url);
    cache.delete(userId);
  }
}
