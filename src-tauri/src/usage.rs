use crate::{agents, codex, storage::Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn estimate_cost_usd(
    provider_kind: &str,
    model: &str,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
) -> f64 {
    let lower = model.to_lowercase();
    let (in_per_m, cached_per_m, out_per_m) = match provider_kind {
        "codex" => {
            if lower.contains("mini") || lower.contains("spark") {
                (0.25, 0.0625, 2.0)
            } else {
                (1.75, 0.44, 14.0)
            }
        }
        "deepseek" => (0.27, 0.07, 1.10),
        "gemini" => {
            if lower.contains("flash") {
                (0.15, 0.0375, 0.60)
            } else {
                (1.25, 0.3125, 10.0)
            }
        }
        _ => (1.0, 0.25, 4.0),
    };
    let cached = cached_tokens.max(0).min(input_tokens.max(0));
    let non_cached = (input_tokens - cached).max(0);
    (non_cached as f64 * in_per_m
        + cached as f64 * cached_per_m
        + output_tokens.max(0) as f64 * out_per_m)
        / 1_000_000.0
}

fn infer_scope_kind(connection: &Connection, conversation_id: &str) -> &'static str {
    if conversation_id.is_empty() {
        return "private";
    }
    let is_group: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1) OR EXISTS(SELECT 1 FROM group_sessions WHERE id=?1)",
            [conversation_id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if is_group {
        "group"
    } else {
        "private"
    }
}

pub fn record(
    database: &Path,
    agent_id: &str,
    provider_kind: &str,
    model: &str,
    conversation_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    status: &str,
) {
    let Ok(connection) = (Store {
        path: database.to_path_buf(),
    })
    .connect() else {
        return;
    };
    let scope_kind = infer_scope_kind(&connection, conversation_id);
    let cost_usd = estimate_cost_usd(
        provider_kind,
        model,
        input_tokens,
        output_tokens,
        cached_tokens,
    );
    let _ = connection.execute(
        "INSERT INTO usage_records(id, agent_id, provider_kind, model, conversation_id, scope_kind, input_tokens, output_tokens, cached_tokens, cost_usd, status, created_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            uuid::Uuid::new_v4().to_string(),
            agent_id,
            provider_kind,
            model,
            conversation_id,
            scope_kind,
            input_tokens.max(0),
            output_tokens.max(0),
            cached_tokens.max(0),
            cost_usd,
            status,
            now_unix(),
        ],
    );
}

fn classify_window(window_mins: i64) -> (&'static str, &'static str) {
    if window_mins > 0 && window_mins <= 360 {
        ("five_hour", "5 小时滚动限额")
    } else if window_mins >= 1440 {
        ("weekly", "每周限额")
    } else {
        ("custom", "周期限额")
    }
}

fn parse_window(raw: &Value) -> Option<Value> {
    if raw.is_null() {
        return None;
    }
    let used_pct = raw["usedPercent"].as_f64()?;
    let window_mins = raw["windowDurationMins"].as_i64().unwrap_or(0);
    let resets_at = raw["resetsAt"].as_i64();
    let remaining_pct = (100.0 - used_pct).clamp(0.0, 100.0);
    let (kind, label) = classify_window(window_mins);
    Some(json!({
        "kind": kind,
        "label": label,
        "usedPercent": used_pct,
        "remainingPercent": remaining_pct,
        "windowDurationMins": window_mins,
        "resetsAt": resets_at,
    }))
}

pub fn parse_codex_rate_limits(raw: &Value) -> Value {
    let rl = &raw["rateLimits"];
    let mut windows = Vec::new();
    if let Some(w) = parse_window(&rl["primary"]) {
        windows.push(w);
    }
    if let Some(w) = parse_window(&rl["secondary"]) {
        windows.push(w);
    }
    let reset_credits = raw["rateLimitResetCredits"]["availableCount"]
        .as_i64()
        .unwrap_or(0);
    json!({
        "available": !rl.is_null(),
        "planType": rl["planType"].as_str(),
        "windows": windows,
        "credits": rl["credits"],
        "resetCreditsCount": reset_credits,
    })
}

pub fn dashboard(store: &Store, codex_rt: &codex::Runtime, days: u32) -> Result<Value, String> {
    let range_days = days.clamp(1, 90) as i64;
    let now = now_unix();
    let since = now - range_days * 86_400;
    let today_since = now - (now.rem_euclid(86_400));

    let agent_list = agents::list(store)?;
    let codex_ready = agent_list
        .iter()
        .any(|a| a.provider_kind == "codex" && a.enabled && a.availability == "ready");

    let codex_limits = if codex_ready {
        match codex_rt.rate_limits(store) {
            Ok(raw) => parse_codex_rate_limits(&raw),
            Err(err) => json!({
                "available": false,
                "error": err,
                "windows": [],
                "resetCreditsCount": 0,
            }),
        }
    } else {
        json!({
            "available": false,
            "windows": [],
            "resetCreditsCount": 0,
        })
    };

    let connection = store.connect()?;

    // Aggregate summary for the selected range
    let (total_cost, total_in, total_out, total_cached, total_turns, group_turns, private_turns): (
        f64,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
    ) = connection
        .query_row(
            "SELECT
                COALESCE(SUM(cost_usd), 0.0),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cached_tokens), 0),
                COUNT(*),
                COALESCE(SUM(CASE WHEN scope_kind='group' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN scope_kind!='group' THEN 1 ELSE 0 END), 0)
             FROM usage_records
             WHERE created_at >= ?1",
            [since],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;

    let cache_hit_rate = if total_in > 0 {
        (total_cached as f64 / total_in as f64 * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    // Daily series (last min(range_days, 14) days)
    let chart_days = range_days.min(14);
    let mut daily_map: BTreeMap<String, [f64; 3]> = BTreeMap::new();
    let mut daily_turns_map: BTreeMap<String, [i64; 3]> = BTreeMap::new();
    for offset in (0..chart_days).rev() {
        let day_ts = now - offset * 86_400;
        let date_label: String = connection
            .query_row(
                "SELECT strftime('%m-%d', ?1, 'unixepoch', 'localtime')",
                [day_ts],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| String::from("--"));
        daily_map.entry(date_label.clone()).or_insert([0.0, 0.0, 0.0]);
        daily_turns_map.entry(date_label).or_insert([0, 0, 0]);
    }

    let mut daily_stmt = connection
        .prepare(
            "SELECT
                strftime('%m-%d', created_at, 'unixepoch', 'localtime') AS d,
                provider_kind,
                COALESCE(SUM(cost_usd), 0.0),
                COUNT(*)
             FROM usage_records
             WHERE created_at >= ?1
             GROUP BY d, provider_kind",
        )
        .map_err(|e| e.to_string())?;
    let daily_rows = daily_stmt
        .query_map([now - (chart_days - 1) * 86_400 - now.rem_euclid(86_400)], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in daily_rows.flatten() {
        let idx = match row.1.as_str() {
            "codex" => 0,
            "deepseek" => 1,
            "gemini" => 2,
            _ => continue,
        };
        if let Some(entry) = daily_map.get_mut(&row.0) {
            entry[idx] += row.2;
        }
        if let Some(entry) = daily_turns_map.get_mut(&row.0) {
            entry[idx] += row.3;
        }
    }

    let daily: Vec<Value> = daily_map
        .into_iter()
        .map(|(date, costs)| {
            let turns = daily_turns_map.get(&date).copied().unwrap_or([0, 0, 0]);
            json!({
                "date": date,
                "codexCost": costs[0],
                "deepseekCost": costs[1],
                "geminiCost": costs[2],
                "codexTurns": turns[0],
                "deepseekTurns": turns[1],
                "geminiTurns": turns[2],
            })
        })
        .collect();

    // Model breakdown
    let mut model_stmt = connection
        .prepare(
            "SELECT
                provider_kind,
                agent_id,
                model,
                COUNT(*),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cached_tokens), 0),
                COALESCE(SUM(cost_usd), 0.0)
             FROM usage_records
             WHERE created_at >= ?1
             GROUP BY provider_kind, agent_id, model
             ORDER BY SUM(cost_usd) DESC, COUNT(*) DESC",
        )
        .map_err(|e| e.to_string())?;
    let models: Vec<Value> = model_stmt
        .query_map([since], |r| {
            let input: i64 = r.get(4)?;
            let cached: i64 = r.get(6)?;
            let cache_pct = if input > 0 {
                (cached as f64 / input as f64 * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            Ok(json!({
                "providerKind": r.get::<_, String>(0)?,
                "agentId": r.get::<_, String>(1)?,
                "model": r.get::<_, String>(2)?,
                "turns": r.get::<_, i64>(3)?,
                "inputTokens": input,
                "outputTokens": r.get::<_, i64>(5)?,
                "cachedTokens": cached,
                "cacheHitRate": cache_pct,
                "costUsd": r.get::<_, f64>(7)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // Per-agent summary
    let mut agent_stats: HashMap<String, (i64, i64, i64, i64, i64, f64, Option<i64>)> =
        HashMap::new();
    let mut agent_stmt = connection
        .prepare(
            "SELECT
                agent_id,
                COALESCE(SUM(CASE WHEN created_at >= ?2 THEN 1 ELSE 0 END), 0),
                COUNT(*),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cached_tokens), 0),
                COALESCE(SUM(cost_usd), 0.0),
                MAX(CASE WHEN status='quota_exhausted' THEN created_at ELSE NULL END)
             FROM usage_records
             WHERE created_at >= ?1
             GROUP BY agent_id",
        )
        .map_err(|e| e.to_string())?;
    let agent_rows = agent_stmt
        .query_map(params![since, today_since], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, f64>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                ),
            ))
        })
        .map_err(|e| e.to_string())?;
    for (id, stats) in agent_rows.flatten() {
        agent_stats.insert(id, stats);
    }

    let agents_summary: Vec<Value> = agent_list
        .into_iter()
        .map(|a| {
            let s = agent_stats
                .get(&a.id)
                .copied()
                .unwrap_or((0, 0, 0, 0, 0, 0.0, None));
            let quota_exhausted_recent = s.6.is_some_and(|ts| now - ts < 5 * 3600)
                || a.error
                    .as_deref()
                    .is_some_and(|e| e.contains("quota_exhausted"));
            json!({
                "id": a.id,
                "displayName": a.display_name,
                "providerKind": a.provider_kind,
                "runtimeKind": a.runtime_kind,
                "connected": a.enabled && a.availability == "ready",
                "availability": a.availability,
                "turnsToday": s.0,
                "turnsRange": s.1,
                "inputTokens": s.2,
                "outputTokens": s.3,
                "cachedTokens": s.4,
                "costUsd": s.5,
                "quotaExhaustedRecent": quota_exhausted_recent,
                "lastQuotaErrorAt": s.6,
            })
        })
        .collect();

    Ok(json!({
        "updatedAt": now,
        "rangeDays": range_days,
        "codexLimits": codex_limits,
        "summary": {
            "totalCostUsd": total_cost,
            "totalInputTokens": total_in,
            "totalOutputTokens": total_out,
            "totalCachedTokens": total_cached,
            "cacheHitRate": cache_hit_rate,
            "totalTurns": total_turns,
            "groupTurns": group_turns,
            "privateTurns": private_turns,
        },
        "daily": daily,
        "models": models,
        "agents": agents_summary,
    }))
}

pub fn estimate_context_window(provider_kind: &str, model: &str) -> i64 {
    let lower = model.to_lowercase();
    match provider_kind {
        "codex" => {
            if lower.contains("gpt-4.1") {
                1_047_576
            } else if lower.contains("o3") || lower.contains("o4") {
                200_000
            } else {
                272_000
            }
        }
        "gemini" => 1_048_576,
        "deepseek" => {
            if lower.contains("64k") {
                65_536
            } else {
                128_000
            }
        }
        _ => 128_000,
    }
}

pub fn conversation_context(
    store: &Store,
    conversation_id: &str,
    members: &[String],
    sessions: &serde_json::Map<String, Value>,
    codex_runtime: Option<&codex::Runtime>,
) -> Result<Value, String> {
    let conn = store.connect()?;
    let mut result = serde_json::Map::new();
    for member in members {
        let provider_kind = agents::provider(store, member).unwrap_or_else(|_| member.clone());
        let session_obj = sessions.get(member);
        let session_model = session_obj
            .and_then(|s| s.get("model"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let thread_id = session_obj
            .and_then(|s| s.get("thread_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let runtime_conv_id: Option<String> = conn
            .query_row(
                "SELECT runtime_conversation_id FROM agent_sessions WHERE agent_id=?1 AND conversation_id=?2",
                params![member, conversation_id],
                |r| r.get(0),
            )
            .ok();
        let alt_conv_id = runtime_conv_id.as_deref().unwrap_or(conversation_id);

        // Query latest non-zero usage record for this conversation (logical or runtime id) and agent.
        let latest_recorded: Option<(String, i64, i64, i64)> = conn
            .query_row(
                "SELECT model, input_tokens, output_tokens, cached_tokens
                 FROM usage_records
                 WHERE (conversation_id=?1 OR conversation_id=?2) AND (agent_id=?3 OR provider_kind=?4)
                   AND (input_tokens > 0 OR output_tokens > 0 OR cached_tokens > 0)
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT 1",
                params![conversation_id, alt_conv_id, member, provider_kind],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .ok();

        let live = if provider_kind == "codex" && !thread_id.is_empty() {
            codex_runtime.and_then(|rt| rt.live_token_usage(thread_id))
        } else {
            None
        };

        let (model, input_tokens, output_tokens, cached_tokens, used_tokens, context_window, source) =
            if let Some(live) = live {
                let model = if !session_model.is_empty() {
                    session_model
                } else {
                    latest_recorded
                        .as_ref()
                        .map(|r| r.0.clone())
                        .unwrap_or_default()
                };
                let window = live
                    .context_window
                    .unwrap_or_else(|| estimate_context_window(&provider_kind, &model));
                let used = live.total_tokens.max(live.input_tokens + live.output_tokens);
                (
                    model,
                    live.input_tokens,
                    live.output_tokens,
                    live.cached_tokens,
                    used,
                    window,
                    "live",
                )
            } else if let Some((rec_model, inp, out, cached)) = latest_recorded {
                let model = if !session_model.is_empty() {
                    session_model
                } else {
                    rec_model
                };
                let window = estimate_context_window(&provider_kind, &model);
                // Gemini ACP reports cachedReadTokens separately from non-cached inputTokens.
                let used = if provider_kind == "gemini" {
                    (inp + cached + out).max(0)
                } else {
                    (inp + out).max(0)
                };
                (model, inp, out, cached, used, window, "recorded")
            } else {
                let window = estimate_context_window(&provider_kind, &session_model);
                (session_model, 0, 0, 0, 0, window, "none")
            };

        let percent = if context_window > 0 && used_tokens > 0 {
            (((used_tokens as f64 / context_window as f64) * 1000.0).round() / 10.0).min(100.0)
        } else {
            0.0
        };

        result.insert(
            member.clone(),
            json!({
                "agentId": member,
                "providerKind": provider_kind,
                "model": model,
                "usedTokens": used_tokens,
                "contextWindow": context_window,
                "percent": percent,
                "inputTokens": input_tokens,
                "outputTokens": output_tokens,
                "cachedTokens": cached_tokens,
                "source": source,
            }),
        );
    }
    Ok(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_weekly_only_without_five_hour() {
        let raw = json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": {
                    "usedPercent": 7,
                    "windowDurationMins": 10080,
                    "resetsAt": 1796013869
                },
                "secondary": null,
                "planType": "prolite"
            },
            "rateLimitResetCredits": {
                "availableCount": 3
            }
        });
        let parsed = parse_codex_rate_limits(&raw);
        assert_eq!(parsed["available"], true);
        assert_eq!(parsed["planType"], "prolite");
        assert_eq!(parsed["resetCreditsCount"], 3);
        let windows = parsed["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0]["kind"], "weekly");
        assert_eq!(windows[0]["remainingPercent"], 93.0);
    }

    #[test]
    fn records_and_aggregates_usage() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        record(
            &store.path,
            "codex",
            "codex",
            "gpt-5.3-codex",
            "codex-personal",
            1000,
            500,
            200,
            "completed",
        );
        record(
            &store.path,
            "gemini",
            "gemini",
            "gemini-3-pro",
            "gemini-personal",
            0,
            0,
            0,
            "quota_exhausted",
        );
        let conn = store.connect().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
