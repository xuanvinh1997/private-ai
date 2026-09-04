//! The SQLite store: session lifecycle, chunk packing, forking, and the read rules.

use std::sync::Arc;

use pai_session::{
    AssistantChunk, AssistantMessage, Message, NewSession, SessionError, SessionEvent,
    SessionService, SessionStore, SessionTitler, SqliteSessionStore, StepEnd, StepStart, TurnEnd,
    TurnEndReason, TurnStart,
};

fn service() -> SessionService {
    let store = SqliteSessionStore::open_in_memory().expect("mở kho");
    SessionService::new(Arc::new(store))
}

fn chunk(turn: u64, step: u64, text: &str) -> SessionEvent {
    SessionEvent::AssistantChunk(AssistantChunk {
        turn,
        step,
        chunk: serde_json::json!({ "kind": "text", "index": 0, "text": text }),
    })
}

fn assistant(text: &str) -> SessionEvent {
    SessionEvent::AssistantMessage(AssistantMessage {
        turn: 0,
        step: 0,
        message: Message::assistant(text),
        usage: None,
        interrupted: None,
    })
}

/// One complete turn, exactly as the agent loop would write it.
async fn mot_luot(session: &mut pai_session::Session, turn: u64, hoi: &str, dap: &str) {
    session
        .append(SessionEvent::TurnStart(TurnStart { turn }))
        .await
        .expect("turn/start");
    session
        .append(SessionEvent::StepStart(StepStart { turn, step: 0 }))
        .await
        .expect("step/start");
    session
        .append_surface(SessionEvent::UserMessage(Message::user(hoi)))
        .await
        .expect("hỏi");
    for word in dap.split(' ') {
        session.append(chunk(turn, 0, word)).await.expect("mảnh");
    }
    session.append_surface(assistant(dap)).await.expect("đáp");
    session
        .append(SessionEvent::StepEnd(StepEnd { turn, step: 0 }))
        .await
        .expect("step/end");
    session
        .append(SessionEvent::TurnEnd(TurnEnd {
            turn,
            reason: TurnEndReason::Completed,
        }))
        .await
        .expect("turn/end");
}

#[tokio::test]
async fn tao_liet_ke_mo_lai() {
    let sessions = service();
    let mut session = sessions
        .create(NewSession::in_dir("/tmp/repo"))
        .await
        .expect("tạo");
    let id = session.id().to_owned();
    mot_luot(&mut session, 0, "chào", "chào bạn").await;
    session.flush().await.expect("ghi");

    let listed = sessions.list(None).await.expect("liệt kê");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].cwd.as_deref(), Some("/tmp/repo"));

    let reopened = sessions.open(&id).await.expect("mở lại");
    assert_eq!(
        reopened.log().len(),
        session.log().len(),
        "đọc lại đủ từng sự kiện"
    );
    for (before, after) in session.log().events().iter().zip(reopened.log().events()) {
        assert_eq!(
            before, after,
            "sổ phải giống nhau từng byte sau khi đi vòng qua đĩa"
        );
    }
    assert_eq!(reopened.derive_messages(), session.derive_messages());
}

#[tokio::test]
async fn nhieu_manh_stream_di_chung_mot_hang() {
    let sessions = service();
    let mut session = sessions.create(NewSession::default()).await.expect("tạo");
    let id = session.id().to_owned();

    session
        .append(SessionEvent::TurnStart(TurnStart { turn: 0 }))
        .await
        .expect("turn/start");
    for i in 0..300 {
        session
            .append(chunk(0, 0, &format!("t{i}")))
            .await
            .expect("mảnh");
    }
    session
        .append_surface(assistant("xong"))
        .await
        .expect("đáp");
    session
        .append(SessionEvent::TurnEnd(TurnEnd {
            turn: 0,
            reason: TurnEndReason::Completed,
        }))
        .await
        .expect("turn/end");
    session.flush().await.expect("ghi");

    let rows = sessions.store().row_count(&id).await.expect("đếm hàng");
    assert_eq!(session.log().len(), 303);
    // 300 chunks pack into three rows (the pending window is 100), plus three ordinary events.
    assert!(rows <= 8, "300 mảnh phải gói lại còn vài hàng, gặp {rows}");

    // Unpacking must reproduce every seq and every timestamp exactly.
    let reopened = sessions.open(&id).await.expect("mở lại");
    assert_eq!(reopened.log().len(), 303);
    for (before, after) in session.log().events().iter().zip(reopened.log().events()) {
        assert_eq!(before.seq, after.seq);
        assert_eq!(
            before.time, after.time,
            "mốc thời gian dựng lại từ hiệu phải khớp"
        );
        assert_eq!(
            before.event, after.event,
            "ranh giới token là dữ liệu, không được nối lại"
        );
    }
}

#[tokio::test]
async fn lo_ghi_lech_seq_bi_tu_choi() {
    let sessions = service();
    let session = sessions.create(NewSession::default()).await.expect("tạo");
    let store = sessions.store().clone();

    let lech = pai_session::SessionEventEnvelope {
        seq: 4,
        time: 0,
        event: SessionEvent::TurnStart(TurnStart { turn: 0 }),
        ignorable: None,
        source_event_seqs: None,
        surface_op: None,
    };
    match store.append(session.id(), vec![lech]).await {
        Err(SessionError::SeqGap {
            expected: 0,
            found: 4,
        }) => {}
        other => panic!("phải là SeqGap, gặp {:?}", other.err()),
    }
}

#[tokio::test]
async fn fork_cam_cat_giua_mot_luot_dang_mo() {
    let sessions = service();
    let mut session = sessions.create(NewSession::default()).await.expect("tạo");
    mot_luot(&mut session, 0, "một", "hai").await;
    // The second turn is left open: turn/start with no turn/end.
    session
        .append(SessionEvent::TurnStart(TurnStart { turn: 1 }))
        .await
        .expect("turn/start");
    session
        .append_surface(SessionEvent::UserMessage(Message::user("ba")))
        .await
        .expect("ba");
    session.flush().await.expect("ghi");

    let boundary = session.log().next_seq() - 1;
    match sessions.fork(session.id(), Some(boundary)).await {
        Err(SessionError::OpenTurn { turn: 1, .. }) => {}
        other => panic!("phải là OpenTurn, gặp {:?}", other.err()),
    }
    // No rounding back to the nearest `turn/end`: only one session ever exists.
    assert_eq!(sessions.list(None).await.expect("liệt kê").len(), 1);
}

#[tokio::test]
async fn fork_tai_ranh_gioi_dong_thi_ke_thua_hat_giong() {
    let sessions = service();
    let mut session = sessions
        .create(NewSession::in_dir("/tmp/repo"))
        .await
        .expect("tạo");
    mot_luot(&mut session, 0, "một", "hai").await;
    mot_luot(&mut session, 1, "ba", "bốn").await;
    session.flush().await.expect("ghi");

    let cuoi_luot_dau = session
        .log()
        .events()
        .iter()
        .find(|e| matches!(&e.event, SessionEvent::TurnEnd(t) if t.turn == 0))
        .expect("turn/end của lượt đầu")
        .seq;

    let child = sessions
        .fork(session.id(), Some(cuoi_luot_dau))
        .await
        .expect("fork");
    assert_eq!(child.header().parent_session.as_deref(), Some(session.id()));
    assert_eq!(child.header().seed_length, Some(cuoi_luot_dau + 1));
    assert_eq!(
        child.header().cwd.as_deref(),
        Some("/tmp/repo"),
        "kế thừa thư mục"
    );
    assert_eq!(child.log().len() as u64, cuoi_luot_dau + 1);
    // The seed keeps its seqs, so new events continue rather than being renumbered.
    assert_eq!(child.log().next_seq(), cuoi_luot_dau + 1);
    assert_eq!(
        child.derive_messages(),
        session.derive_messages()[..2].to_vec(),
        "phiên con thấy đúng phần lịch sử trước ranh giới"
    );
}

#[tokio::test]
async fn fork_ranh_gioi_qua_xa_bao_loi_chu_khong_lam_tron() {
    let sessions = service();
    let mut session = sessions.create(NewSession::default()).await.expect("tạo");
    mot_luot(&mut session, 0, "một", "hai").await;
    session.flush().await.expect("ghi");

    match sessions.fork(session.id(), Some(9_999)).await {
        Err(SessionError::InvalidBoundary {
            boundary: 9_999, ..
        }) => {}
        other => panic!("phải là InvalidBoundary, gặp {:?}", other.err()),
    }
}

#[tokio::test]
async fn luot_mo_coi_duoc_dong_lai_chu_khong_bi_cat_cut() {
    let sessions = service();
    let mut session = sessions.create(NewSession::default()).await.expect("tạo");
    session
        .append(SessionEvent::TurnStart(TurnStart { turn: 0 }))
        .await
        .expect("turn/start");
    session
        .append_surface(SessionEvent::UserMessage(Message::user("dở dang")))
        .await
        .expect("hỏi");
    session.flush().await.expect("ghi");
    let truoc = session.log().len();

    // Another process reopens after the crash.
    let healed = sessions.open(session.id()).await.expect("mở lại");
    assert_eq!(healed.log().len(), truoc + 1, "ghi thêm, không cắt cụt");
    match &healed.log().events()[truoc].event {
        SessionEvent::TurnEnd(t) => {
            assert_eq!(t.turn, 0);
            assert_eq!(t.reason, TurnEndReason::Interrupted);
        }
        other => panic!("phải là turn/end interrupted, gặp {other:?}"),
    }
    // Once closed, forking at the end of the log is valid again.
    sessions
        .fork(healed.id(), None)
        .await
        .expect("fork sau khi đã đóng");
}

#[tokio::test]
async fn loai_su_kien_la_khong_ignorable_thi_tu_choi_ca_so() {
    let store = SqliteSessionStore::open_in_memory().expect("mở kho");
    let sessions = SessionService::new(Arc::new(store));
    let session = sessions.create(NewSession::default()).await.expect("tạo");
    let id = session.id().to_owned();

    // Simulate a newer build having written into this log.
    let tu_tuong_lai = pai_session::SessionEventEnvelope {
        seq: 0,
        time: 0,
        event: SessionEvent::Unknown(pai_session::UnknownEvent {
            kind: "compaction/summary".into(),
            data: serde_json::json!({ "summary": "…" }),
        }),
        ignorable: None,
        source_event_seqs: None,
        surface_op: None,
    };
    sessions
        .store()
        .append(&id, vec![tu_tuong_lai])
        .await
        .expect("ghi");

    match sessions.open(&id).await {
        Err(SessionError::FormatUnsupported(kind)) => assert_eq!(kind, "compaction/summary"),
        other => panic!("phải từ chối, gặp {:?}", other.err().map(|e| e.to_string())),
    }
}

#[tokio::test]
async fn loai_su_kien_la_co_the_bo_qua_thi_doc_duoc() {
    let store = SqliteSessionStore::open_in_memory().expect("mở kho");
    let sessions = SessionService::new(Arc::new(store));
    let session = sessions.create(NewSession::default()).await.expect("tạo");
    let id = session.id().to_owned();

    let tu_tuong_lai = pai_session::SessionEventEnvelope {
        seq: 0,
        time: 7,
        event: SessionEvent::Unknown(pai_session::UnknownEvent {
            kind: "telemetry/ping".into(),
            data: serde_json::json!({ "ms": 3 }),
        }),
        ignorable: Some(true),
        source_event_seqs: None,
        surface_op: None,
    };
    sessions
        .store()
        .append(&id, vec![tu_tuong_lai.clone()])
        .await
        .expect("ghi");

    let reopened = sessions.open(&id).await.expect("đọc được");
    assert_eq!(
        reopened.log().events()[0],
        tu_tuong_lai,
        "giữ nguyên văn để ghi lại được"
    );
    assert!(reopened.derive_messages().is_empty());
}

#[tokio::test]
async fn tu_choi_mo_mot_tep_sqlite_khong_phai_cua_minh() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = dir.path().join("nguoi-khac.db");
    {
        let conn = rusqlite::Connection::open(&path).expect("tạo tệp");
        conn.execute_batch("PRAGMA application_id = 12345; CREATE TABLE t (x);")
            .expect("ghi tệp lạ");
    }
    match SqliteSessionStore::open(&path) {
        Err(SessionError::NotOurStore { found: 12345 }) => {}
        other => panic!("phải từ chối, gặp {:?}", other.err().map(|e| e.to_string())),
    }
}

#[tokio::test]
async fn mo_lai_mot_tep_that_giu_duoc_du_lieu() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = dir.path().join("sessions.db");
    let id;
    {
        let sessions = SessionService::new(Arc::new(SqliteSessionStore::open(&path).expect("mở")));
        let mut session = sessions
            .create(NewSession::in_dir("/repo"))
            .await
            .expect("tạo");
        id = session.id().to_owned();
        mot_luot(&mut session, 0, "một", "hai").await;
        session.flush().await.expect("ghi");
        session.set_title("Lượt đầu").await.expect("đặt tên");
    }
    let sessions = SessionService::new(Arc::new(SqliteSessionStore::open(&path).expect("mở lại")));
    let session = sessions.open(&id).await.expect("mở lại phiên");
    assert_eq!(session.header().title.as_deref(), Some("Lượt đầu"));
    assert_eq!(session.derive_messages().len(), 2);
}

#[tokio::test]
async fn seam_tieu_de_co_dung_mot_provider_va_no_tra_none() {
    let log = pai_session::SessionLog::new();
    let titler: Arc<dyn SessionTitler> = Arc::new(pai_session::NoTitle);
    assert_eq!(titler.title(&log).await.expect("hỏi tiêu đề"), None);
}

#[tokio::test]
async fn plugin_cam_ca_hai_seam_vao_cay() {
    use pai_core::{Context, Plugin};
    use pai_session::{SessionPlugin, SessionTitle, Sessions};

    let root = Context::root();
    let ctx = root.plugin("pai-session");
    let store: Arc<dyn SessionStore> =
        Arc::new(SqliteSessionStore::open_in_memory().expect("mở kho"));
    SessionPlugin::new(store).apply(&ctx).await.expect("cắm");

    assert!(root.get::<Sessions>().is_some());
    assert!(root.get::<SessionTitle>().is_some());
    ctx.effects().dispose().await;
    assert!(
        root.get::<Sessions>().is_none(),
        "gỡ plugin là thu hồi đăng ký"
    );
}

#[tokio::test]
async fn xoa_phien_thi_su_kien_cua_no_di_theo() {
    let store = SqliteSessionStore::open_in_memory().expect("mở kho");
    let service = SessionService::new(Arc::new(store));

    let mut session = service
        .create(NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");
    let id = session.id().to_string();
    session
        .append_surface(SessionEvent::UserMessage(Message::user("chào")))
        .await
        .expect("ghi được");
    session.flush().await.expect("đẩy xuống đĩa");
    drop(session);

    service.delete(&id).await.expect("xoá được");
    assert!(
        service.open(&id).await.is_err(),
        "phiên đã xoá mà vẫn mở được"
    );
    // Events must follow: this tests the schema's `ON DELETE CASCADE`, so dropping the cascade turns this red.
    assert!(
        service
            .store()
            .load(&id)
            .await
            .map(|events| events.is_empty())
            .unwrap_or(true),
        "sự kiện còn sót lại sau khi phiên đã bị xoá"
    );
    // Deleting a missing session is a named error, not a silent no-op, so a double-clicked Delete says so.
    assert!(service.delete(&id).await.is_err());
}

#[tokio::test]
async fn doi_ten_phien_thi_danh_sach_thay_ngay() {
    let store = SqliteSessionStore::open_in_memory().expect("mở kho");
    let service = SessionService::new(Arc::new(store));
    let session = service
        .create(NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");
    let id = session.id().to_string();
    drop(session);

    service
        .rename(&id, "Sửa bộ nạp cấu hình")
        .await
        .expect("đổi tên được");
    let listed = service.list(Some(10)).await.expect("liệt kê");
    let found = listed
        .iter()
        .find(|header| header.id == id)
        .expect("có trong danh sách");
    assert_eq!(found.title.as_deref(), Some("Sửa bộ nạp cấu hình"));
}

#[tokio::test]
async fn dong_phu_lay_cau_cuoi_cung_va_bo_qua_phien_chua_noi_gi() {
    let store = SqliteSessionStore::open_in_memory().expect("mở kho");
    let service = SessionService::new(Arc::new(store));

    let mut noisy = service
        .create(NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");
    let talked = noisy.id().to_string();
    for text in ["câu đầu", "câu giữa", "câu cuối cùng"] {
        noisy
            .append_surface(SessionEvent::UserMessage(Message::user(text)))
            .await
            .expect("ghi được");
    }
    noisy.flush().await.expect("đẩy xuống đĩa");
    drop(noisy);

    let quiet = service
        .create(NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");
    let silent = quiet.id().to_string();
    drop(quiet);

    let previews = service
        .previews(&[talked.clone(), silent.clone()])
        .await
        .expect("lấy được dòng phụ");

    assert_eq!(
        previews.get(&talked).map(String::as_str),
        Some("câu cuối cùng")
    );
    // A session with nothing said is absent rather than an empty string: no subtitle reads fine, a blank one does not.
    assert!(!previews.contains_key(&silent));
}

#[tokio::test]
async fn dong_phu_cat_cau_dai_va_lam_phang_xuong_dong() {
    let store = SqliteSessionStore::open_in_memory().expect("mở kho");
    let service = SessionService::new(Arc::new(store));
    let mut session = service
        .create(NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");
    let id = session.id().to_string();

    let long = format!("dòng một\ndòng hai {}", "x".repeat(300));
    session
        .append_surface(SessionEvent::UserMessage(Message::user(&long)))
        .await
        .expect("ghi được");
    session.flush().await.expect("đẩy xuống đĩa");
    drop(session);

    let previews = service
        .previews(std::slice::from_ref(&id))
        .await
        .expect("lấy được");
    let line = previews.get(&id).expect("có dòng phụ");
    // Truncate in the store, not the UI: a long answer per row is bandwidth spent on text cut at render time.
    assert!(
        line.chars().count() <= 161,
        "dài {} ký tự",
        line.chars().count()
    );
    assert!(line.ends_with('…'));
    assert!(
        !line.contains('\n'),
        "xuống dòng làm vỡ hàng một dòng: {line:?}"
    );
}
