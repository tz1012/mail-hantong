use jiff::{civil::DateTime, tz::TimeZone};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TimezoneError {
    Invalid,
    AmbiguousOrSkipped,
}

pub(crate) fn validate_id(id: &str) -> Result<(), TimezoneError> {
    if id.is_empty()
        || id.len() > 128
        || id.starts_with('+')
        || id.starts_with('-')
        || id.starts_with("POSIX/")
        || id.contains('\0')
    {
        return Err(TimezoneError::Invalid);
    }
    TimeZone::get(id)
        .map(|_| ())
        .map_err(|_| TimezoneError::Invalid)
}

#[allow(dead_code)]
pub(crate) fn midnight_timestamp(
    id: &str,
    year: i16,
    month: i8,
    day: i8,
) -> Result<i64, TimezoneError> {
    validate_id(id)?;
    let tz = TimeZone::get(id).map_err(|_| TimezoneError::Invalid)?;
    let civil = DateTime::new(year, month, day, 0, 0, 0, 0).map_err(|_| TimezoneError::Invalid)?;
    tz.to_ambiguous_zoned(civil)
        .unambiguous()
        .map(|z| z.timestamp().as_second())
        .map_err(|_| TimezoneError::AmbiguousOrSkipped)
}
