//! 地图库（移植自 Python `maplib.py`）
//!
//! 管理已生成的地图：索引 + 布局缓存 + 容量控制。
//!
//! 三条设计要点（都是 Python 侧踩坑后定下来的）：
//!   1. **索引只存元数据**，图片/布局本体另存，不复制（省内存）
//!   2. **读-改-写必须整体加锁** —— Python 侧曾因多线程并发覆盖，索引从 190 条悄悄掉到 65 条
//!   3. **索引可从文件自愈**：`scan()` 把「文件存在但没登记」的补回来（索引只是缓存，数据本体才是关键）
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// 默认容量上限：300 MB（超出按 LRU 清理）
pub const DEFAULT_MAX_BYTES: u64 = 300 * 1024 * 1024;

/// 一次清理最多回报多少条"可清理项"的明细（`removed` 仍是全量）。
/// 50 条足够用户在确认弹窗里看懂"要删什么"，再多就只是把 IPC 包撑大。
pub const VICTIMS_MAX: usize = 50;

/// 干跑开关的解析（**破坏性接口的纪律**）。
///
/// `dry_run` → `dry` → **默认 `true`**：想真删必须自己把 `false` 写出来。
/// 这条纪律是拿事故换的 —— Python 侧在真实删除路径上误删过 119 张地图缓存。
///
/// 抽成纯函数的理由：命令层要 `AppHandle`（手机上跑不了单测），
/// 而"默认值是不是 true"恰恰是**最需要被测到**的那一行。
pub fn resolve_dry_run(dry_run: Option<bool>, dry: Option<bool>) -> bool {
    dry_run.or(dry).unwrap_or(true)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 地图库
pub struct MapLib {
    root: PathBuf,
    index_file: PathBuf,
    layout_dir: PathBuf,
    max_bytes: u64,
    /// 保护「读索引 → 改 → 写回」整个事务（不是只锁读写单步）
    lock: Mutex<()>,
}

impl MapLib {
    /// `root` 一般是 `<data_dir>/game_data/world_map/maplib`
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let layout_dir = root.join("layouts");
        let _ = std::fs::create_dir_all(&layout_dir);
        Self {
            index_file: root.join("index.json"),
            root,
            layout_dir,
            max_bytes: DEFAULT_MAX_BYTES,
            lock: Mutex::new(()),
        }
    }

    pub fn with_max_bytes(mut self, bytes: u64) -> Self {
        self.max_bytes = bytes;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn guard(&self) -> MutexGuard<'_, ()> {
        // 锁中毒（某次持锁 panic）时也继续：地图库是缓存，宁可继续也不整体挂掉
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn load(&self) -> Value {
        if let Ok(txt) = std::fs::read_to_string(&self.index_file) {
            if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                if v.get("entries").and_then(|e| e.as_object()).is_some() {
                    return v;
                }
            }
        }
        json!({"version": 1, "entries": {}})
    }

    fn save(&self, d: &Value) -> std::io::Result<()> {
        let tmp = self.index_file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(d).unwrap_or_default())?;
        std::fs::rename(&tmp, &self.index_file) // 原子替换
    }

    /// 登记一张地图（按路径去重；文件不存在则忽略）
    pub fn register(&self, rel_path: &str, kind: &str, adcode: &str, style: &str, meta: Value) -> Option<String> {
        let _g = self.guard();
        let ap = self.abs(rel_path);
        let st = std::fs::metadata(&ap).ok()?;
        let eid = format!("{kind}:{}:{}", if adcode.is_empty() { rel_path } else { adcode }, style);
        let mut d = self.load();
        let entries = d.get_mut("entries").and_then(|e| e.as_object_mut())?;

        // 同一文件只保留一条：按 path 去重，继承更早的 createdAt 与更高 version
        let dup: Vec<String> = entries
            .iter()
            .filter(|(k, v)| v.get("path").and_then(|p| p.as_str()) == Some(rel_path) && k.as_str() != eid)
            .map(|(k, _)| k.clone())
            .collect();
        let mut created = now();
        let mut version = 1u64;
        for k in dup {
            if let Some(prev) = entries.remove(&k) {
                if let Some(c) = prev.get("createdAt").and_then(|v| v.as_u64()) {
                    created = created.min(c);
                }
                if let Some(v) = prev.get("version").and_then(|v| v.as_u64()) {
                    version = version.max(v);
                }
            }
        }
        if let Some(old) = entries.get(&eid) {
            if let Some(c) = old.get("createdAt").and_then(|v| v.as_u64()) {
                created = c;
            }
            version = old.get("version").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
        }
        entries.insert(
            eid.clone(),
            json!({
                "id": eid, "kind": kind, "adcode": adcode, "style": style,
                "path": rel_path, "bytes": st.len(),
                "mtime": st.modified().ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()).unwrap_or(0),
                "createdAt": created, "lastAccess": now(), "version": version,
                "origin": "generated", "meta": meta,
            }),
        );
        let _ = self.save(&d);
        Some(eid)
    }

    /// 标记一次访问（LRU 用）
    pub fn touch(&self, eid: &str) -> bool {
        let _g = self.guard();
        let mut d = self.load();
        let hit = d
            .get_mut("entries")
            .and_then(|e| e.as_object_mut())
            .and_then(|e| e.get_mut(eid))
            .map(|e| {
                if let Some(o) = e.as_object_mut() {
                    o.insert("lastAccess".into(), json!(now()));
                }
                true
            })
            .unwrap_or(false);
        if hit {
            let _ = self.save(&d);
        }
        hit
    }

    /// 取一条（连带清理已消失的文件）
    pub fn get(&self, eid: &str) -> Option<Value> {
        let _g = self.guard();
        let mut d = self.load();
        let path = d
            .get("entries")
            .and_then(|e| e.get(eid))
            .and_then(|e| e.get("path"))
            .and_then(|p| p.as_str())
            .map(|s| s.to_string())?;
        if !self.abs(&path).exists() {
            if let Some(entries) = d.get_mut("entries").and_then(|e| e.as_object_mut()) {
                entries.remove(eid);
            }
            let _ = self.save(&d);
            return None;
        }
        let mut e = d.get("entries")?.get(eid)?.clone();
        if let Some(o) = e.as_object_mut() {
            o.insert("lastAccess".into(), json!(now()));
        }
        let _ = self.save(&d);
        Some(e)
    }

    /// 列表（可按 kind/adcode 过滤；sort ∈ recent|oldest|largest）
    pub fn list(&self, kind: Option<&str>, adcode: Option<&str>, limit: Option<usize>, sort: &str) -> Vec<Value> {
        let _g = self.guard();
        self.list_locked(kind, adcode, limit, sort)
    }

    /// 内部版：调用方必须已持锁（Mutex 非重入，嵌套加锁会自死锁）
    fn list_locked(&self, kind: Option<&str>, adcode: Option<&str>, limit: Option<usize>, sort: &str) -> Vec<Value> {
        let d = self.load();
        let mut alive: Vec<Value> = Vec::new();
        let mut dropped = false;
        if let Some(entries) = d.get("entries").and_then(|e| e.as_object()) {
            for (_, e) in entries {
                let path = e.get("path").and_then(|p| p.as_str()).unwrap_or("");
                if self.abs(path).exists() {
                    alive.push(e.clone());
                } else {
                    dropped = true;
                }
            }
        }
        if dropped {
            let keep: Map<String, Value> = alive
                .iter()
                .filter_map(|e| Some((e.get("id")?.as_str()?.to_string(), e.clone())))
                .collect();
            let _ = self.save(&json!({"version": 1, "entries": keep}));
        }
        if let Some(k) = kind {
            alive.retain(|e| e.get("kind").and_then(|v| v.as_str()) == Some(k));
        }
        if let Some(a) = adcode {
            alive.retain(|e| e.get("adcode").and_then(|v| v.as_str()) == Some(a));
        }
        match sort {
            "oldest" => alive.sort_by_key(|e| e.get("createdAt").and_then(|v| v.as_u64()).unwrap_or(0)),
            "largest" => alive.sort_by_key(|e| std::cmp::Reverse(e.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0))),
            _ => alive.sort_by_key(|e| std::cmp::Reverse(e.get("lastAccess").and_then(|v| v.as_u64()).unwrap_or(0))),
        }
        if let Some(n) = limit {
            alive.truncate(n);
        }
        alive
    }

    /// 删除（可选连同文件）
    pub fn delete(&self, eid: &str, remove_file: bool) -> bool {
        let _g = self.guard();
        self.delete_locked(eid, remove_file)
    }

    /// 内部版：调用方必须已持锁
    fn delete_locked(&self, eid: &str, remove_file: bool) -> bool {
        let mut d = self.load();
        let path = d
            .get("entries")
            .and_then(|e| e.get(eid))
            .and_then(|e| e.get("path"))
            .and_then(|p| p.as_str())
            .map(|s| s.to_string());
        let removed = d
            .get_mut("entries")
            .and_then(|e| e.as_object_mut())
            .map(|e| e.remove(eid).is_some())
            .unwrap_or(false);
        if !removed {
            return false;
        }
        let _ = self.save(&d);
        if remove_file {
            if let Some(p) = path {
                let _ = std::fs::remove_file(self.abs(&p));
            }
        }
        true
    }

    /// 统计
    pub fn stats(&self) -> Value {
        let _g = self.guard();
        self.stats_locked()
    }

    /// 内部版：调用方必须已持锁
    fn stats_locked(&self) -> Value {
        let items = self.list_locked(None, None, None, "recent");
        let mut by_kind: Map<String, Value> = Map::new();
        let mut total: u64 = 0;
        for e in &items {
            let k = e.get("kind").and_then(|v| v.as_str()).unwrap_or("other").to_string();
            let c = by_kind.get(&k).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
            by_kind.insert(k, json!(c));
            total += e.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        }
        json!({
            "count": items.len(), "bytes": total,
            "mb": ((total as f64 / 1048576.0) * 100.0).round() / 100.0,
            "by_kind": by_kind,
            "max_bytes": self.max_bytes,
            "max_mb": if self.max_bytes > 0 { ((self.max_bytes as f64 / 1048576.0) * 100.0).round() / 100.0 } else { 0.0 },
        })
    }

    /// 容量控制（LRU）。
    ///
    /// **务必先 `dry_run = true` 看会删什么** —— Python 侧就是在这里误删过 119 张图。
    ///
    /// 返回（**干跑与真删同形**，前端不用分两套读法）：
    /// ```text
    /// { removed, freed, freed_mb, dry_run,
    ///   victims: [id…],            // ≤50 条，前端既有契约
    ///   victims_detail: [ {id,kind,adcode,style,bytes,path,lastAccess,reason} … ],
    ///   victims_truncated: bool,   // 超过 50 条时为 true（removed 仍是全量）
    ///   stats }                    // 只有真删才附（干跑附了会让人以为已经生效）
    /// ```
    /// `victims` 在干跑时是"**将要**删的"，真删时是"**确实**删掉的"
    /// （`delete_locked` 返回 false 的条目不会被算进去）。
    pub fn enforce_limit(&self, max_bytes: Option<u64>, dry_run: bool) -> Value {
        let _g = self.guard();
        let limit = max_bytes.unwrap_or(self.max_bytes);
        if limit == 0 {
            // 0 = 不设上限（`max_bytes` 的既有语义），不是"清空"
            return json!({
                "removed": 0, "freed": 0, "freed_mb": 0.0, "dry_run": dry_run,
                "victims": [], "victims_detail": [], "victims_truncated": false,
            });
        }
        let items = self.list_locked(None, None, None, "recent"); // 最近访问在前
        let mut total: u64 = items.iter().map(|e| e.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0)).sum();
        let mut removed = 0usize;
        let mut freed: u64 = 0;
        let mut victims: Vec<String> = Vec::new();
        let mut details: Vec<Value> = Vec::new();
        for e in items.iter().rev() {
            if total <= limit {
                break;
            }
            let sz = e.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let eid = e.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let detail = json!({
                "id": eid,
                "kind": e.get("kind").cloned().unwrap_or(json!("other")),
                "adcode": e.get("adcode").cloned().unwrap_or(json!("")),
                "style": e.get("style").cloned().unwrap_or(json!("")),
                "bytes": sz,
                "path": e.get("path").cloned().unwrap_or(json!("")),
                "lastAccess": e.get("lastAccess").cloned().unwrap_or(json!(0)),
                // 只有一种淘汰原因：LRU（最久没访问的先走）
                "reason": "lru",
            });
            if dry_run {
                victims.push(eid);
                details.push(detail);
                removed += 1;
                freed += sz;
                total -= sz;
                continue;
            }
            if self.delete_locked(&eid, true) {
                victims.push(eid);
                details.push(detail);
                removed += 1;
                freed += sz;
                total -= sz;
            }
        }
        let truncated = victims.len() > VICTIMS_MAX;
        victims.truncate(VICTIMS_MAX);
        details.truncate(VICTIMS_MAX);
        let mut out = json!({
            "removed": removed, "freed": freed,
            "freed_mb": ((freed as f64 / 1048576.0) * 100.0).round() / 100.0,
            "dry_run": dry_run,
            "victims": victims,
            "victims_detail": details,
            "victims_truncated": truncated,
        });
        if !dry_run {
            out["stats"] = self.stats_locked();
        }
        out
    }

    // ───────── 布局缓存（AI 生成的小区布局，可复用省 LLM 调用）─────────

    pub fn layout_key(area: &str, context: &str, expand: i32) -> String {
        let d = md5::compute(format!("{area}|{context}|{expand}").as_bytes());
        d.0.iter().take(6).map(|b| format!("{b:02x}")).collect()
    }

    pub fn save_layout(&self, key: &str, layout: &Value, meta: Value) -> Option<PathBuf> {
        let rec = json!({"key": key, "savedAt": now(), "meta": meta, "layout": layout});
        let fp = self.layout_dir.join(format!("{key}.json"));
        std::fs::write(&fp, serde_json::to_string(&rec).ok()?).ok()?;
        Some(fp)
    }

    pub fn get_layout(&self, key: &str) -> Option<Value> {
        let txt = std::fs::read_to_string(self.layout_dir.join(format!("{key}.json"))).ok()?;
        let v: Value = serde_json::from_str(&txt).ok()?;
        v.get("layout").filter(|l| l.is_object()).cloned()
    }

    pub fn list_layouts(&self, limit: Option<usize>) -> Vec<Value> {
        let mut out: Vec<(u64, Value)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.layout_dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !name.ends_with(".json") {
                    continue;
                }
                let Ok(txt) = std::fs::read_to_string(e.path()) else { continue };
                let Ok(v) = serde_json::from_str::<Value>(&txt) else { continue };
                let saved = v.get("savedAt").and_then(|x| x.as_u64()).unwrap_or(0);
                out.push((
                    saved,
                    json!({
                        "key": name.trim_end_matches(".json"),
                        "savedAt": saved,
                        "meta": v.get("meta").cloned().unwrap_or(json!({})),
                        "name": v.get("layout").and_then(|l| l.get("name")).cloned().unwrap_or(json!("")),
                        "buildings": v.get("layout").and_then(|l| l.get("buildings"))
                            .and_then(|b| b.as_array()).map(|a| a.len()).unwrap_or(0),
                    }),
                ));
            }
        }
        out.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
        if let Some(n) = limit {
            out.truncate(n);
        }
        out.into_iter().map(|(_, v)| v).collect()
    }

    fn abs(&self, rel: &str) -> PathBuf {
        let p = Path::new(rel);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            // 相对路径按「项目根」解析：root 的祖父（…/world_map/maplib → …/world_map）
            self.root
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join(rel))
                .unwrap_or_else(|| p.to_path_buf())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 测试用的临时项目根（相当于真实布局里的 world_map/ 目录）
    struct Tmp(PathBuf);
    impl Tmp {
        /// Android/Termux 上 /tmp 不可写：优先 TMPDIR，退到 $HOME/.cache/wm_test
        fn base() -> PathBuf {
            if let Ok(t) = std::env::var("TMPDIR") {
                let p = PathBuf::from(t);
                if p.is_dir() {
                    return p;
                }
            }
            let p = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                .join(".cache")
                .join("wm_test");
            let _ = std::fs::create_dir_all(&p);
            p
        }
        fn new(tag: &str) -> Self {
            let d = Self::base().join(format!("wm_maplib_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            // 与真实布局一致：<项目根>/worlddata/maplib（所以 maplib 的祖父才是项目根）
            std::fs::create_dir_all(d.join("worlddata").join("maplib")).unwrap();
            std::fs::create_dir_all(d.join("images")).unwrap();
            Tmp(d)
        }
        fn lib(&self) -> MapLib {
            MapLib::new(self.0.join("worlddata").join("maplib"))
        }
        fn png(&self, name: &str, size: usize) -> String {
            let rel = format!("images/{name}");
            std::fs::write(self.0.join(&rel), vec![0u8; size]).unwrap();
            rel
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn register_get_delete_roundtrip() {
        let t = Tmp::new("rt");
        let lib = t.lib();
        let rel = t.png("440100_gaode.png", 2000);
        let id = lib.register(&rel, "region", "440100", "gaode", json!({"name":"广州市"})).unwrap();
        assert_eq!(id, "region:440100:gaode");
        let e = lib.get(&id).unwrap();
        assert_eq!(e["bytes"], json!(2000));
        assert_eq!(e["meta"]["name"], json!("广州市"));
        assert!(lib.delete(&id, true));
        assert!(lib.get(&id).is_none());
        assert!(!lib.abs(&rel).exists(), "删条目应连同文件");
    }

    #[test]
    fn register_is_idempotent_by_path() {
        let t = Tmp::new("dup");
        let lib = t.lib();
        let rel = t.png("x_gaode.png", 100);
        let _ = lib.register(&rel, "region", "440100", "gaode", json!({}));
        let id2 = lib.register(&rel, "region", "440100", "gaode", json!({})).unwrap();
        // 同一路径重复登记不该出现两条，且 version 递增
        assert_eq!(lib.list(None, None, None, "recent").len(), 1);
        assert_eq!(lib.get(&id2).unwrap()["version"], json!(2));
    }

    #[test]
    fn missing_file_is_dropped_from_list() {
        let t = Tmp::new("gone");
        let lib = t.lib();
        let rel = t.png("a.png", 100);
        let rel2 = t.png("b.png", 100);
        lib.register(&rel, "region", "1", "gaode", json!({}));
        lib.register(&rel2, "region", "2", "gaode", json!({}));
        assert_eq!(lib.list(None, None, None, "recent").len(), 2);
        std::fs::remove_file(lib.abs(&rel)).unwrap();
        assert_eq!(lib.list(None, None, None, "recent").len(), 1, "文件没了应从索引剔除");
    }

    #[test]
    fn filter_and_sort() {
        let t = Tmp::new("fs");
        let lib = t.lib();
        for (i, (ad, style, sz)) in [("1", "gaode", 100usize), ("1", "dark", 300), ("2", "gaode", 200)]
            .iter()
            .enumerate()
        {
            let rel = t.png(&format!("{ad}_{style}.png"), *sz);
            lib.register(&rel, "region", ad, style, json!({}));
            std::thread::sleep(std::time::Duration::from_millis(5));
            let _ = i;
        }
        assert_eq!(lib.list(Some("region"), Some("1"), None, "recent").len(), 2);
        let biggest = lib.list(None, None, None, "largest");
        assert_eq!(biggest[0]["bytes"], json!(300));
        assert_eq!(lib.list(None, None, Some(1), "recent").len(), 1);
    }

    #[test]
    fn stats_counts_by_kind() {
        let t = Tmp::new("st");
        let lib = t.lib();
        for (k, n) in [("region", 2), ("district", 1)] {
            for i in 0..n {
                let rel = t.png(&format!("{k}_{i}.png"), 1000);
                lib.register(&rel, k, &format!("{k}{i}"), "gaode", json!({}));
            }
        }
        let s = lib.stats();
        assert_eq!(s["count"], json!(3));
        assert_eq!(s["by_kind"]["region"], json!(2));
        assert_eq!(s["bytes"], json!(3000));
    }

    #[test]
    fn enforce_limit_dry_run_never_deletes() {
        let t = Tmp::new("dry");
        let lib = t.lib();
        for i in 0..4 {
            let rel = t.png(&format!("f{i}.png"), 2000);
            lib.register(&rel, "region", &format!("{i}"), "gaode", json!({}));
        }
        let r = lib.enforce_limit(Some(1), true);
        assert_eq!(r["dry_run"], json!(true));
        assert_eq!(r["removed"], json!(4), "上限 1 字节时四张都该进候选");
        assert!(r["victims"].as_array().unwrap().len() == 4);
        assert_eq!(lib.list(None, None, None, "recent").len(), 4, "干跑绝不能删东西");
        assert!(lib.abs("images/f0.png").exists());
    }

    #[test]
    fn enforce_limit_removes_until_under_cap() {
        let t = Tmp::new("real");
        let lib = t.lib();
        for i in 0..4 {
            let rel = t.png(&format!("g{i}.png"), 2000);
            lib.register(&rel, "region", &format!("{i}"), "gaode", json!({}));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let r = lib.enforce_limit(Some(4000), false);
        assert_eq!(r["removed"], json!(2), "8000 字节要清到 ≤4000，需删 2 张");
        let total: u64 = lib.list(None, None, None, "recent").iter()
            .map(|e| e["bytes"].as_u64().unwrap_or(0)).sum();
        assert!(total <= 4000, "清理后总量应达标，实际 {total}");
    }

    // ══════════════════════════════════════════════════════════════
    //  P5-4：占用读数 + 手动清理（**破坏性接口默认干跑**）
    // ══════════════════════════════════════════════════════════════

    /// `dry_run` 的默认值必须是 **true**：参数一个都不给时绝不能真删。
    /// 这是整个 P5-4 里最要命的一行（Python 侧就是在这儿误删过 119 张图）。
    #[test]
    fn dry_run_defaults_to_true_in_every_shape() {
        assert!(resolve_dry_run(None, None), "两个参数都不给 → 必须干跑");
        assert!(resolve_dry_run(Some(true), None));
        assert!(resolve_dry_run(None, Some(true)));
        // 显式 false 才是真删（dry_run 优先于旧名 dry）
        assert!(!resolve_dry_run(Some(false), None));
        assert!(!resolve_dry_run(None, Some(false)));
        assert!(!resolve_dry_run(Some(false), Some(true)));
        assert!(resolve_dry_run(Some(true), Some(false)), "dry_run 优先");
    }

    /// 干跑：把"会删什么"说清楚（张数 / 字节 / 明细），但**一个文件都不许动**。
    #[test]
    fn dry_run_reports_victims_without_touching_anything() {
        let t = Tmp::new("dry2");
        let lib = t.lib();
        let mut rels = Vec::new();
        for i in 0..3 {
            let rel = t.png(&format!("d{i}.png"), 1000);
            lib.register(&rel, "region", &format!("44{i:04}"), "gaode", json!({}));
            std::thread::sleep(std::time::Duration::from_millis(5));
            rels.push(rel);
        }
        let before = lib.stats();
        // 上限 1000 字节 → 3000 字节要删到 ≤1000，至少两张进候选
        let r = lib.enforce_limit(Some(1000), true);
        assert_eq!(r["dry_run"], json!(true));
        assert!(r["removed"].as_u64().unwrap() > 0, "必须给出会删几张: {r}");
        assert!(r["freed"].as_u64().unwrap() > 0, "必须给出会释放多少字节: {r}");
        assert_eq!(r["victims_truncated"], json!(false));

        let victims: Vec<String> = r["victims"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(!victims.is_empty(), "干跑必须列出『哪些』: {r}");
        // 明细：每条都带 kind/bytes/path，且 bytes 之和 == freed
        let details = r["victims_detail"].as_array().unwrap();
        assert_eq!(details.len(), victims.len());
        let sum: u64 = details.iter().map(|d| d["bytes"].as_u64().unwrap_or(0)).sum();
        assert_eq!(sum, r["freed"].as_u64().unwrap(), "明细字节之和要等于 freed: {r}");
        for d in details {
            assert!(d["id"].as_str().is_some());
            assert_eq!(d["kind"], json!("region"));
            assert_eq!(d["reason"], json!("lru"));
            assert!(d["path"].as_str().is_some_and(|p| !p.is_empty()));
            assert!(d["lastAccess"].as_u64().is_some());
        }

        // 干跑的底线：索引没变、文件全在、统计一模一样
        assert_eq!(lib.stats(), before, "干跑不许改变任何统计");
        assert_eq!(lib.list(None, None, None, "recent").len(), 3);
        for rel in &rels {
            assert!(lib.abs(rel).exists(), "干跑删了文件: {rel}");
        }
        // 干跑**不**附 stats：附了会让人以为已经生效
        assert!(r.get("stats").is_none());
    }

    /// 真删：只删干跑列出来的那些（一个不多、一个不少），其余文件与索引都留着。
    #[test]
    fn real_cleanup_deletes_exactly_the_listed_victims() {
        let t = Tmp::new("real2");
        let lib = t.lib();
        let mut rels = Vec::new();
        for i in 0..4 {
            let rel = t.png(&format!("e{i}.png"), 1000);
            lib.register(&rel, "district", &format!("44{i:04}"), "dark", json!({}));
            std::thread::sleep(std::time::Duration::from_millis(5));
            rels.push(rel);
        }
        // ① 先干跑拿到"会删哪些"（这正是用户点确认前看到的那份名单）
        let plan = lib.enforce_limit(Some(2000), true);
        let victims: Vec<String> = plan["victims"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(!victims.is_empty());
        // ② 用户点了确认 → 真删
        let done = lib.enforce_limit(Some(2000), false);
        assert_eq!(done["dry_run"], json!(false));
        assert_eq!(done["removed"], json!(victims.len()), "真删数量应与名单一致");
        assert_eq!(done["victims"], plan["victims"], "真删回报的名单应与干跑一致");
        assert!(done["stats"].is_object(), "真删后附最新统计，前端不用再查一次");

        let alive = lib.list(None, None, None, "recent");
        for (i, rel) in rels.iter().enumerate() {
            let id = format!("district:44{i:04}:dark");
            let was_victim = victims.iter().any(|v| *v == id);
            assert_eq!(
                lib.abs(rel).exists(),
                !was_victim,
                "文件 {rel} 的去留必须与名单一致（was_victim={was_victim}）"
            );
            assert_eq!(alive.iter().any(|e| e["id"] == json!(id)), !was_victim);
        }
        let total: u64 = alive.iter().map(|e| e["bytes"].as_u64().unwrap_or(0)).sum();
        assert!(total <= 2000, "真删后应降到上限以内，实际 {total}");
    }

    /// 没超上限时：一件都不删（别把"清理"做成"定时清空"）。
    #[test]
    fn cleanup_is_a_no_op_when_under_the_cap() {
        let t = Tmp::new("under");
        let lib = t.lib();
        let rel = t.png("keep.png", 1000);
        lib.register(&rel, "region", "440100", "gaode", json!({}));
        let r = lib.enforce_limit(Some(10 * 1024 * 1024), false);
        assert_eq!(r["removed"], json!(0));
        assert_eq!(r["freed"], json!(0));
        assert_eq!(r["victims"], json!([]));
        assert!(lib.abs(&rel).exists());
    }

    /// `max_bytes = 0` 的既有语义是"不设上限"（不是"清空"）——
    /// 这条一旦反过来，就是又一次 119 张的事故。
    #[test]
    fn zero_cap_means_unlimited_not_delete_everything() {
        let t = Tmp::new("zero");
        let lib = t.lib();
        let rel = t.png("z.png", 5000);
        lib.register(&rel, "region", "440100", "gaode", json!({}));
        let r = lib.enforce_limit(Some(0), false);
        assert_eq!(r["removed"], json!(0));
        assert!(lib.abs(&rel).exists(), "0 = 不限容量，绝不能当成清空");
        assert_eq!(r["victims"], json!([]));
    }

    #[test]
    fn layout_cache_roundtrip() {
        let t = Tmp::new("lay");
        let lib = t.lib();
        let k = MapLib::layout_key("广州市·越秀区", "测试", 0);
        assert!(lib.get_layout(&k).is_none());
        lib.save_layout(&k, &json!({"name":"越秀小区","size":20,"buildings":[1,2,3]}), json!({"area":"越秀"}));
        let got = lib.get_layout(&k).unwrap();
        assert_eq!(got["name"], json!("越秀小区"));
        let ls = lib.list_layouts(None);
        assert_eq!(ls.len(), 1);
        assert_eq!(ls[0]["buildings"], json!(3));
        // key 必须稳定（同输入同 key，才能命中缓存）
        assert_eq!(k, MapLib::layout_key("广州市·越秀区", "测试", 0));
        assert_ne!(k, MapLib::layout_key("广州市·越秀区", "别的剧情", 0));
    }

    #[test]
    fn concurrent_register_does_not_lose_entries() {
        // Python 侧就是因为没锁，索引从 190 条掉到 65 条
        use std::sync::Arc;
        let t = Tmp::new("race");
        let lib = Arc::new(t.lib());
        let mut rels = Vec::new();
        for i in 0..20 {
            rels.push(t.png(&format!("c{i}.png"), 100));
        }
        let mut handles = Vec::new();
        for (i, rel) in rels.into_iter().enumerate() {
            let lib2 = Arc::clone(&lib);
            handles.push(std::thread::spawn(move || {
                lib2.register(&rel, "region", &format!("{i}"), "gaode", json!({}));
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(lib.list(None, None, None, "recent").len(), 20,
                   "并发登记 20 条必须一条不少（这正是加锁要防的）");
    }
}
