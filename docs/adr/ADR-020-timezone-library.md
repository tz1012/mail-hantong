# ADR-020 IANA 시간대와 현지 날짜 해석

상태: 2026-10-29 연속 개발 위임에 따른 구현 결정. M1-05와 후속 검색 기간 해석에 적용한다. 사람의 제품·보안 검토를 대체하지 않는다.

## 결정

Rust 1.95 제품의 IANA 시간대 검증과 civil 날짜·시간을 instant로 바꾸는 공통 라이브러리로 `jiff` **0.2.38**을 고정한다. 사용 후보 기능은 다음과 같다.

```toml
jiff = { version = "=0.2.38", default-features = false,
         features = ["std", "tz-fat", "tzdb-bundle-always"] }
```

`tzdb-bundle-always`를 선택하는 이유는 Windows에만 시간대 DB가 내장되는 기본 설정 대신 Windows·macOS·Linux가 같은 IANA 규칙을 사용하게 하기 위해서다. 운영체제의 현재 시간대 자동 발견은 설정값 검증에 사용하지 않는다. 사용자 설정의 `timezone_id`를 `jiff::tz::TimeZone::get`으로 조회하고 실패하면 `INVALID_INPUT`으로 처리한다. tzdb 업데이트는 Cargo.lock과 라이브러리 버전을 함께 갱신하는 릴리스 작업으로 한다.

기간 표현을 civil 자정으로 해석할 때는 Jiff의 편의 메서드 `Date::in_tz`/`DateTime::in_tz`를 사용하지 않는다. 이 메서드들은 기본 `Disambiguation::Compatible`로 gap/fold를 자동 보정한다. 검색 계약이 요구하는 모호한·존재하지 않는 날짜 거부를 지키려면 `TimeZone::to_ambiguous_zoned(...).unambiguous()` 또는 동등한 `Disambiguation::Reject` 경로를 사용하고, 오류를 `INVALID_INPUT`으로 매핑한다. 예시는 다음과 같다.

```rust
let tz = jiff::tz::TimeZone::get(timezone_id)?;
let civil = jiff::civil::date(year, month, day).at(0, 0, 0, 0);
let zoned = tz.to_ambiguous_zoned(civil).unambiguous()?;
let instant = zoned.timestamp();
```

이 규칙은 DST 자정 이동과 일반적인 1시간 DST 전환뿐 아니라 역사적·정치적 offset 변경도 같은 방식으로 다룬다. 파서나 쿼리 엔진이 Jiff의 기본 `Compatible` 경로를 직접 호출하지 못하도록 날짜 해석을 작은 내부 함수로 감싼다.

## 비교

`jiff` 0.2.38은 2026-10-06 릴리스이며 내장 tzdb가 2026e로 갱신됐다. MSRV는 1.70이고 MIT 또는 Unlicense로 배포된다. 공식 문서는 Windows에서 IANA DB를 라이브러리에 자동 내장하고, `tzdb-bundle-platform`은 Windows에서만 번들을 넣는다고 설명한다. `TimeZone::to_ambiguous_zoned`와 `unambiguous`는 fold/gap를 자동 선택하지 않고 오류로 만들 수 있다. 프로젝트의 Rust 1.95와 Windows 우선·후속 macOS 목표에 맞는다. Jiff 자체 문서는 1.0 시점이 아직 정해지지 않았고 0.2는 1.0 이후 1년간 중요 수정만 계획한다고 명시하므로, 외부 API에 Jiff 타입을 노출하지 않고 내부 adapter를 둔다.

대안인 `chrono` 0.4.45 + `chrono-tz` 0.10.4도 MIT/Apache-2.0이고 MSRV가 각각 1.62/1.65다. `chrono-tz`의 `MappedLocalTime`은 `Single`, `Ambiguous(earliest, latest)`, `None(gap/error)`를 제공해 거부 정책을 구현할 수 있다. 그러나 시간대 전체를 생성하는 빌드 구조이며 공식 crate 페이지의 0.10.4는 2025-07-11 릴리스, 소스 패키지 약 8.4 MB·평균 빌드 약 1분 3초로 표시된다. `filter-by-regex`는 빌드 크기를 줄이지만 사용자가 선택할 수 있는 임의 IANA ID를 제한하므로 이 제품의 설정 계약에는 부적합하다. Chrono 자체는 시간대 데이터를 기본 제공하지 않고 companion crate를 요구한다.

속도 우열은 이 자료만으로 단정하지 않는다. Jiff는 성능을 중요한 목표로 삼지만 “reasonable performance”를 명시하고, Chrono-TZ의 문서 수치도 제품 실행시간이 아니라 crate 빌드 지표다. M2-08 성능 카드에서 실제 메일 검색·기간 해석을 같은 데이터로 측정한다.

## 위험과 완화

- Jiff 0.x API가 1.0 전에 바뀔 수 있다. `hantong-domain`의 내부 시간대 모듈에서만 Jiff 타입을 사용하고 외부 계약에는 RFC3339/epoch와 문자열 IANA ID만 노출한다.
- `tzdb-bundle-always`는 모든 대상에 tzdb를 포함해 바이너리 크기를 늘릴 수 있다. 동일한 규칙과 오프라인 Windows 동작을 우선하고, 릴리스 빌드 크기는 M9-18에서 측정한다.
- IANA 규칙은 법률 변경으로 갱신된다. 버전과 tzdb 릴리스를 lockfile·릴리스 기록에 남기고, 과거 예약/검색 결과에는 해석 당시의 timezone ID와 instant를 저장한다.
- `Date::in_tz`의 기본 Compatible 보정을 실수로 사용하면 날짜 범위가 조용히 이동한다. 내부 API는 `unambiguous()`만 허용하고 gap/fold 회귀 시험을 추가한다.

## 근거

- [Jiff 0.2.38 crate와 MSRV·라이선스·기능](https://docs.rs/crate/jiff/0.2.38)
- [Jiff 기능과 Windows tzdb 번들](https://docs.rs/jiff/latest/jiff/)
- [Jiff `TimeZone::to_ambiguous_zoned`](https://docs.rs/jiff/latest/jiff/tz/struct.TimeZone.html)
- [Jiff `Disambiguation::Reject`](https://docs.rs/jiff/latest/jiff/tz/enum.Disambiguation.html)
- [Jiff 0.2.38 변경 내역과 tzdb 2026e](https://docs.rs/crate/jiff/0.2.38/source/CHANGELOG.md)
- [Chrono 0.4.45 문서·MSRV·시간대 데이터 경계](https://docs.rs/crate/chrono/0.4.45)
- [Chrono-TZ 0.10.4 Cargo manifest·라이선스·기능](https://raw.githubusercontent.com/chronotope/chrono-tz/v0.10.4/chrono-tz/Cargo.toml)
- [Chrono-TZ `MappedLocalTime` 대안 동작](https://docs.rs/chrono/latest/chrono/offset/enum.LocalResult.html)

