// SPDX-License-Identifier: MIT

use skkserv_compound::dictionary::ParsedEntry;
use skkserv_compound::generator::CompoundGeneratorConfig;
use skkserv_compound::loader::build_snapshot;
use skkserv_compound::server::{
    IncomingCharset, OpcodeResult, SkkServer, extract_messages, sanitize_yomi,
};
use skkserv_compound::store::DictionaryStore;

fn nashi(reading: &str, candidates: &[&str]) -> ParsedEntry {
    ParsedEntry::new(
        reading.to_string(),
        candidates.iter().map(|s| s.to_string()).collect(),
    )
}

fn ari(reading: &str, candidates: &[&str]) -> ParsedEntry {
    ParsedEntry::with_okuri(
        reading.to_string(),
        candidates.iter().map(|s| s.to_string()).collect(),
        true,
    )
}

fn make_server(user: &[(&str, &[&str])], system: &[(&str, &[&str])]) -> SkkServer {
    let u: Vec<ParsedEntry> = user.iter().map(|(r, c)| nashi(r, c)).collect();
    let s: Vec<ParsedEntry> = system.iter().map(|(r, c)| nashi(r, c)).collect();
    let snap = build_snapshot(&u, &s);
    SkkServer::new(
        "test",
        "skkserv-compound",
        DictionaryStore::with_initial(snap),
        CompoundGeneratorConfig::default(),
    )
}

#[tokio::test]
async fn opcode1_returns_candidates() {
    let server = make_server(&[], &[("せいそう", &["清掃"]), ("ぎょうしゃ", &["業者"])]);
    let response = server.candidate_response("せいそうぎょうしゃ ").await;
    assert_eq!(response, "1/清掃業者/\n");
}

#[tokio::test]
async fn opcode1_returns_four_on_miss() {
    let server = make_server(&[], &[]);
    let response = server.candidate_response("そんざいしない ").await;
    assert_eq!(response, "4\n");
}

#[tokio::test]
async fn trims_whitespace() {
    let server = make_server(&[], &[("あ", &["亜"]), ("い", &["胃"])]);
    let response = server.candidate_response("  あい\n").await;
    assert_eq!(response, "1/亜胃/\n");
}

#[tokio::test]
async fn strips_okuri_marker() {
    let server = make_server(&[], &[("あ", &["亜"]), ("い", &["胃"])]);
    let response = server.candidate_response("あいi").await;
    assert_eq!(response, "4\n");
}

#[tokio::test]
async fn opcode1_okuri_ari_compound() {
    let user = vec![
        nashi("もんだい", &["問題"]),
        ari("なs", &["無", "済"]),
    ];
    let snap = build_snapshot(&user, &[]);
    let server = SkkServer::new(
        "test",
        "skkserv-compound",
        DictionaryStore::with_initial(snap),
        CompoundGeneratorConfig::default(),
    );
    let response = server.candidate_response("もんだいなs ").await;
    assert_eq!(response, "1/問題無/問題済/\n");
}

#[tokio::test]
async fn opcode1_ascii_input_does_not_extract_okuri() {
    let server = make_server(&[], &[("ka", &["カ"]), ("waii", &["ワイイ"])]);
    let response = server.candidate_response("kawaii ").await;
    assert_eq!(response, "1/カワイイ/\n");
}

#[tokio::test]
async fn opcode_zero_closes() {
    let server = make_server(&[], &[]);
    assert_eq!(
        server.handle_opcode('0', "", "127.0.0.1", 1178).await,
        OpcodeResult::Close
    );
}

#[tokio::test]
async fn opcode_two_returns_version() {
    let server = SkkServer::new(
        "v1",
        "skkserv-test",
        DictionaryStore::new(),
        CompoundGeneratorConfig::default(),
    );
    assert_eq!(
        server.handle_opcode('2', "", "127.0.0.1", 1178).await,
        OpcodeResult::Reply("skkserv-test/v1 ".to_string())
    );
}

#[tokio::test]
async fn opcode_three_returns_host_port() {
    let server = make_server(&[], &[]);
    let result = server.handle_opcode('3', "", "127.0.0.1", 1178).await;
    match result {
        OpcodeResult::Reply(body) => assert!(body.ends_with("/127.0.0.1:1178 "), "got: {}", body),
        _ => panic!("expected reply"),
    }
}

#[tokio::test]
async fn opcode_four_returns_four() {
    let server = make_server(&[], &[]);
    assert_eq!(
        server.handle_opcode('4', "なにか ", "127.0.0.1", 1178).await,
        OpcodeResult::Reply("4\n".to_string())
    );
}

#[tokio::test]
async fn opcode_one_integrates() {
    let server = make_server(&[], &[("あ", &["亜"]), ("い", &["胃"])]);
    assert_eq!(
        server.handle_opcode('1', "あい ", "127.0.0.1", 1178).await,
        OpcodeResult::Reply("1/亜胃/\n".to_string())
    );
}

#[tokio::test]
async fn unsupported_opcode_ignored() {
    let server = make_server(&[], &[]);
    assert_eq!(
        server.handle_opcode('9', "", "127.0.0.1", 1178).await,
        OpcodeResult::Ignore
    );
}

#[test]
fn decodes_euc_jp() {
    let text = "1あい ";
    let (cow, _enc, had_errors) = encoding_rs::EUC_JP.encode(text);
    assert!(!had_errors);
    let bytes: Vec<u8> = cow.into_owned();
    let decoded = IncomingCharset::EucJp.decode(&bytes).unwrap();
    assert_eq!(decoded, text);
    let operand: String = decoded.chars().skip(1).collect();
    let (body, okuri) = sanitize_yomi(&operand);
    assert_eq!(body, "あい");
    assert_eq!(okuri, None);
}

#[test]
fn extracts_multiple_requests_from_single_buffer() {
    let mut buffer: Vec<u8> = "1あい 1うえ ".as_bytes().to_vec();
    let messages = extract_messages(&mut buffer, IncomingCharset::Utf8);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].0, '1');
    assert_eq!(messages[0].1, "あい");
    assert_eq!(messages[1].0, '1');
    assert_eq!(messages[1].1, "うえ");
    assert!(buffer.is_empty());
}

#[test]
fn reassembles_across_buffers() {
    let full = "1あい ".as_bytes().to_vec();
    let split_at = full.len() / 2;
    let mut buffer: Vec<u8> = full[..split_at].to_vec();
    let first_pass = extract_messages(&mut buffer, IncomingCharset::Utf8);
    assert!(first_pass.is_empty());
    assert!(!buffer.is_empty());
    buffer.extend_from_slice(&full[split_at..]);
    let second_pass = extract_messages(&mut buffer, IncomingCharset::Utf8);
    assert_eq!(second_pass.len(), 1);
    assert_eq!(second_pass[0].0, '1');
    assert_eq!(second_pass[0].1, "あい");
    assert!(buffer.is_empty());
}

#[test]
fn recognizes_both_delimiters() {
    let mut buffer: Vec<u8> = "1あ\n1い ".as_bytes().to_vec();
    let messages = extract_messages(&mut buffer, IncomingCharset::Utf8);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].0, '1');
    assert_eq!(messages[0].1, "あ");
    assert_eq!(messages[1].0, '1');
    assert_eq!(messages[1].1, "い");
}

#[test]
fn keeps_unterminated_tail() {
    let mut buffer: Vec<u8> = "1あい 1うえ".as_bytes().to_vec();
    let messages = extract_messages(&mut buffer, IncomingCharset::Utf8);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].1, "あい");
    assert!(!buffer.is_empty());
}

#[test]
fn extracts_euc_jp_requests() {
    let text = "1あい 1うえ ";
    let (cow, _enc, had_errors) = encoding_rs::EUC_JP.encode(text);
    assert!(!had_errors);
    let mut buffer: Vec<u8> = cow.into_owned();
    let messages = extract_messages(&mut buffer, IncomingCharset::EucJp);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].1, "あい");
    assert_eq!(messages[1].1, "うえ");
}

#[test]
fn skips_empty_payloads() {
    let mut buffer: Vec<u8> = "  1あ ".as_bytes().to_vec();
    let messages = extract_messages(&mut buffer, IncomingCharset::Utf8);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, '1');
    assert_eq!(messages[0].1, "あ");
}

#[test]
fn sanitize_trims_and_strips() {
    let (body, okuri) = sanitize_yomi(" あい \n");
    assert_eq!(body, "あい");
    assert_eq!(okuri, None);

    let (body, okuri) = sanitize_yomi("abc");
    assert_eq!(body, "abc");
    assert_eq!(okuri, None);
}

#[test]
fn sanitize_extracts_okuri_prefix() {
    let (body, okuri) = sanitize_yomi("おくr");
    assert_eq!(body, "おく");
    assert_eq!(okuri, Some("r".to_string()));

    let (body, okuri) = sanitize_yomi("もんだいなs ");
    assert_eq!(body, "もんだいな");
    assert_eq!(okuri, Some("s".to_string()));
}

#[test]
fn sanitize_returns_empty_tuple() {
    let (body, okuri) = sanitize_yomi("   \n");
    assert_eq!(body, "");
    assert_eq!(okuri, None);
}
