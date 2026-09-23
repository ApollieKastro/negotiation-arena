//! Доменный слой: сущности, порты (контракты) и чистая бизнес-логика.
//!
//! Не зависит от I/O: ни HTTP, ни SQLite, ни внешних API.
//! Инфраструктура реализует порты (`domain::ports`) адаптерами.

pub mod entities;
pub mod ports;
pub mod services;
