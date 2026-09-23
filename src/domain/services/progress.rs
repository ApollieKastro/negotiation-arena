//! Прогрессия игрока: XP, уровни, прогресс до следующего уровня.
//!
//! XP начисляется суммой положительных баллов завершённых сессий
//! (см. SQL-агрегат `user_stats_aggregate`). Уровень — чистая функция от XP,
//! без I/O и без хранения в БД: при каждом запросе stats пересчитывается.

/// Пороги уровней: индекс = уровень − 1, значение = минимум XP для уровня.
///
/// 10 уровней: 0 · 100 · 250 · 500 · 850 · 1300 · 1800 · 2400 · 3100 · 3900.
pub const LEVEL_THRESHOLDS: [i64; 10] = [0, 100, 250, 500, 850, 1300, 1800, 2400, 3100, 3900];

/// Максимальный уровень (длина таблицы порогов).
pub const MAX_LEVEL: u32 = LEVEL_THRESHOLDS.len() as u32;

/// Прогресс игрока для UI (карточка уровня + progress bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Progress {
    /// Суммарный XP (положительные баллы завершённых сессий).
    pub xp: i64,
    /// Текущий уровень (1..=MAX_LEVEL).
    pub level: u32,
    /// XP текущего уровня.
    pub level_xp: i64,
    /// XP, необходимый для следующего уровня (0 — максимальный уровень).
    pub next_level_xp: i64,
    /// Прогресс до следующего уровня в процентах (0..=100).
    pub progress_pct: u32,
    /// Сколько XP осталось до следующего уровня (0 — максимум).
    pub xp_to_next: i64,
}

/// Строит [`Progress`] из суммарного XP.
///
/// На последнем уровне `next_level_xp = 0`, `xp_to_next = 0`,
/// `progress_pct = 100`.
pub fn progress_for_xp(xp: i64) -> Progress {
    let xp = xp.max(0);
    let mut level: u32 = 1;
    for (i, &threshold) in LEVEL_THRESHOLDS.iter().enumerate() {
        if xp >= threshold {
            level = (i as u32) + 1;
        } else {
            break;
        }
    }

    let level_start = LEVEL_THRESHOLDS[(level - 1) as usize];
    let is_max = (level as usize) >= LEVEL_THRESHOLDS.len();
    let next_threshold = if is_max {
        level_start
    } else {
        LEVEL_THRESHOLDS[level as usize]
    };

    if is_max {
        Progress {
            xp,
            level,
            level_xp: xp - level_start,
            next_level_xp: 0,
            progress_pct: 100,
            xp_to_next: 0,
        }
    } else {
        let span = (next_threshold - level_start).max(1);
        let into_level = (xp - level_start).clamp(0, span);
        Progress {
            xp,
            level,
            level_xp: into_level,
            next_level_xp: next_threshold,
            progress_pct: ((into_level * 100) / span).clamp(0, 100) as u32,
            xp_to_next: next_threshold - xp,
        }
    }
}

/// XP для завершённой сессии: только положительный балл (отрицательный не учитывается).
pub fn xp_from_score(score: i32) -> i64 {
    i64::from(score.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_1_at_zero_xp() {
        let p = progress_for_xp(0);
        assert_eq!(p.level, 1);
        assert_eq!(p.progress_pct, 0);
        assert_eq!(p.xp_to_next, 100);
        assert_eq!(p.next_level_xp, 100);
    }

    #[test]
    fn level_up_at_threshold() {
        assert_eq!(progress_for_xp(99).level, 1);
        assert_eq!(progress_for_xp(100).level, 2);
        assert_eq!(progress_for_xp(3899).level, 9);
        assert_eq!(progress_for_xp(3900).level, 10);
    }

    #[test]
    fn max_level_progress_is_full() {
        let p = progress_for_xp(5000);
        assert_eq!(p.level, MAX_LEVEL);
        assert_eq!(p.progress_pct, 100);
        assert_eq!(p.xp_to_next, 0);
        assert_eq!(p.next_level_xp, 0);
    }

    #[test]
    fn mid_level_progress_pct() {
        // Уровень 2: 100..250, xp=175 → ровно половина.
        let p = progress_for_xp(175);
        assert_eq!(p.level, 2);
        assert_eq!(p.progress_pct, 50);
        assert_eq!(p.xp_to_next, 75);
        assert_eq!(p.level_xp, 75);
    }

    #[test]
    fn negative_xp_clamped() {
        let p = progress_for_xp(-10);
        assert_eq!(p.xp, 0);
        assert_eq!(p.level, 1);
    }

    #[test]
    fn xp_from_score_ignores_negative() {
        assert_eq!(xp_from_score(80), 80);
        assert_eq!(xp_from_score(0), 0);
        assert_eq!(xp_from_score(-5), 0);
    }

    #[test]
    fn thresholds_are_monotonic() {
        for pair in LEVEL_THRESHOLDS.windows(2) {
            assert!(pair[0] < pair[1], "пороги должны расти: {pair:?}");
        }
    }
}
