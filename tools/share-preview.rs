// Render synthetic fixture data with the unmodified beta.22 product renderer.
// The generator supplies terminal_view.rs from the release source at build time.
mod terminal_view;
use terminal_view::{Frame, Theme, render};

fn main() {
    let accounts = ["개인 메일 · 데모", "업무 메일 · 데모"]
        .map(str::to_owned);
    let folders = ["받은편지함", "보낸편지함", "임시보관함", "휴지통"]
        .map(str::to_owned);
    let rows = [
        "[업무] 금요일 회의 안내",
        "[개인] 주말 산책 약속",
        "[업무] 디자인 시안 공유",
        "[개인] 이번 주 읽을거리",
        "[업무] 프로젝트 진행 소식",
        "[개인] 예약 확인 안내",
        "[업무] 문서 검토 부탁드립니다",
        "[개인] 한통에 오신 걸 환영해요",
    ].map(str::to_owned);
    let body = "제목: 금요일 회의 안내\n보낸 사람: 민지 <minji@example.invalid>\n받는 사람: team@example.invalid\n참조: design@example.invalid\n날짜: 2026-11-04 09:30\n\n안녕하세요.\n\n금요일 오후 2시에 다음 회의를 진행합니다.\n이번 주 작업 내용과 다음 일정을 함께 확인해요.\n\n1. 메일 화면과 키보드 이동\n2. 여러 계정의 통합 받은편지함\n3. 다음 주 일정 정리\n\n확인 후 편한 시간에 답장 부탁드려요.\n감사합니다!\n\n민지 드림\n\n[가상 메일 · 실제 계정이나 발송 없음]";
    print!("{}", render(&Frame {
        lang_ko: true,
        width: std::env::args().nth(1).and_then(|v|v.parse().ok()).unwrap_or(128),
        height: std::env::args().nth(2).and_then(|v|v.parse().ok()).unwrap_or(32),
        theme: Theme::Blue,
        screen: 3, selected: 0, account: "통합 받은편지함 · 계정 태그",
        folder: "Inbox", accounts: &accounts, folders: &folders, rows: &rows,
        body, body_offset: 0, status: "가상 데이터 미리보기 · 계정 태그 / 메일 목록 / 본문", help: false,
    }));
}
