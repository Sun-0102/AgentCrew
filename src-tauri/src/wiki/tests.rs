use super::*;

#[test]
fn self_contained_local_wiki_indexing_search_deprecation_and_links() {
    let dir = tempfile::tempdir().unwrap();
    let wiki_root = dir.path().join("sample-wiki");
    let concepts = wiki_root.join("wiki").join("concepts");
    std::fs::create_dir_all(&concepts).unwrap();

    std::fs::write(
        concepts.join("policy-2026.md"),
        "---\ntitle: 2026 渠道返点政策\nstatus: active\ntags: [返点, 渠道]\n---\n\n# 2026 渠道返点政策\n\n金牌经销商在锁价期内享有阶梯返点，特殊情况见 [[price-lock-exception]]。\n",
    )
    .unwrap();
    std::fs::write(
        concepts.join("policy-2024.md"),
        "---\ntitle: 2024 旧版返点政策\nstatus: deprecated\n---\n\n# 2024 旧版返点政策\n\n金牌经销商旧版返点规则（已废弃）。\n",
    )
    .unwrap();
    std::fs::write(
        concepts.join("price-lock-exception.md"),
        "---\ntitle: 锁价期例外说明\nstatus: active\n---\n\n# 锁价期例外说明\n\nS2 锁价期例外审批流程。\n",
    )
    .unwrap();

    let store = Store::open(dir.path().join("test.sqlite")).unwrap();
    let source = add_local_source(
        &store,
        wiki_root.to_str().unwrap(),
        Some("临时测试知识库"),
        Some("core_wiki"),
        None,
    )
    .unwrap();
    assert_eq!(source.content_subdir, "wiki");
    assert_eq!(source.doc_count, 2);

    let hits = search_fts(
        &store,
        Some(&source.id),
        "金牌经销商 返点",
        None,
        false,
        false,
        10,
        0,
    )
    .unwrap();
    assert!(hits.iter().any(|h| h.rel_path.contains("policy-2026.md")));
    assert!(hits.iter().all(|h| !h.rel_path.contains("policy-2024.md")));

    let doc = read_document(&store, Some(&source.id), "concepts/policy-2026.md", None, None).unwrap();
    assert!(doc
        .links
        .iter()
        .any(|l| l.target == "price-lock-exception" && l.resolved_path.is_some()));
}

#[test]
fn mock_business_wiki_indexing_search_deprecation_and_links() {
    let mock_root = Path::new("/Users/nangua/Documents/AgentCrew/mock-business-wiki");
    if !mock_root.is_dir() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.sqlite")).unwrap();

    let source = add_local_source(
        &store,
        mock_root.to_str().unwrap(),
        Some("星云优品测试库"),
        Some("core_wiki"),
        None,
    )
    .unwrap();
    assert_eq!(source.content_subdir, "wiki");
    assert!(source.doc_count >= 14, "doc_count={}", source.doc_count);

    // Case 1: 飞鹰补发 & 绿灯仓
    let hits1 = search_fts(
        &store,
        Some(&source.id),
        "飞鹰补发 绿灯仓 VIP",
        None,
        false,
        false,
        10,
        0,
    )
    .unwrap();
    assert!(
        hits1.iter().any(|h| h.rel_path.contains("aftersales-refund-sop")
            || h.rel_path.contains("nebula-internal-glossary")),
        "expected aftersales/glossary hit, got {:?}",
        hits1.iter().map(|h| &h.rel_path).collect::<Vec<_>>()
    );

    // Case 2: 返点政策检索必须过滤掉 status: deprecated 的 2024 旧规，且读主规则可看到一跳双链 s2-price-lock-exception
    let hits2 = search_fts(
        &store,
        Some(&source.id),
        "金牌经销商 返点 S2 锁价期",
        None,
        false,
        false,
        10,
        0,
    )
    .unwrap();
    assert!(
        hits2.iter().all(|h| !h.rel_path.contains("legacy-rebate-policy-2024")),
        "deprecated 2024 policy must be excluded from default search"
    );
    assert!(
        hits2.iter().any(|h| h.rel_path.contains("channel-rebate-policy-2026")),
        "expected 2026 rebate policy in hits"
    );
    let doc2026 = read_document(
        &store,
        Some(&source.id),
        "concepts/channel-rebate-policy-2026.md",
        None,
        None,
    )
    .unwrap();
    assert!(
        doc2026
            .links
            .iter()
            .any(|l| l.target == "s2-price-lock-exception" && l.resolved_path.is_some()),
        "expected 1-hop wikilink to s2-price-lock-exception, got {:?}",
        doc2026.links
    );

    // Case 3: 华辰集团 账期 60天
    let hits3 = search_fts(
        &store,
        Some(&source.id),
        "华辰集团 账期 审批",
        None,
        false,
        false,
        10,
        0,
    )
    .unwrap();
    assert!(
        hits3.iter().any(|h| h.rel_path.contains("ka-partner-huachen")
            || h.rel_path.contains("strategic-ka-special-approval")),
        "expected huachen / strategic-ka hit"
    );
}
