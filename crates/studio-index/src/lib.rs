mod extract;
mod lang;
mod schema;
mod walk;

pub use extract::{Extraction, Ref, Symbol};
pub use lang::{is_binary_path, Lang};
pub use schema::SCHEMA_VERSION;
pub use walk::{is_ignored_dir, ScanReport};

use rusqlite::{params, Connection};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, IndexError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    Unchanged,
    Indexed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    pub path: String,
    pub lang: Option<String>,
    pub blake3: String,
    pub size: u64,
    pub mtime: String,
    pub is_binary: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SymbolRecord {
    pub fqname: String,
    pub path: String,
    pub kind: String,
    pub signature: Option<String>,
    pub doc: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub symbol: SymbolRecord,
    pub calls: Vec<String>,
    pub called_by: Vec<String>,
}

pub struct Index {
    conn: Connection,
}

impl Index {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn index_file(&mut self, path: &str, bytes: &[u8], mtime: &str) -> Result<Refresh> {
        let hash = blake3::hash(bytes).to_hex().to_string();

        if self.known_hash(path)?.as_deref() == Some(hash.as_str()) {
            return Ok(Refresh::Unchanged);
        }

        let binary = lang::is_binary_path(path);
        let language = if binary { None } else { Lang::from_path(path) };

        let tx = self.conn.transaction()?;

        tx.execute(
            "INSERT INTO files (path, lang, blake3, size, mtime, is_binary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
               lang = excluded.lang, blake3 = excluded.blake3,
               size = excluded.size, mtime = excluded.mtime,
               is_binary = excluded.is_binary",
            params![
                path,
                language.map(Lang::wire),
                hash,
                bytes.len() as i64,
                mtime,
                binary as i64
            ],
        )?;

        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM symbols_fts WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM refs WHERE path = ?1", params![path])?;

        if let Some(language) = language.filter(|l| l.has_extractor()) {
            if let Ok(source) = std::str::from_utf8(bytes) {
                let found = extract::extract(language, path, source);

                for symbol in &found.symbols {
                    tx.execute(
                        "INSERT INTO symbols
                           (fqname, path, kind, signature, doc, line_start, line_end)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT(fqname, path) DO UPDATE SET
                           kind = excluded.kind, signature = excluded.signature,
                           doc = excluded.doc, line_start = excluded.line_start,
                           line_end = excluded.line_end",
                        params![
                            symbol.fqname,
                            path,
                            symbol.kind,
                            symbol.signature,
                            symbol.doc,
                            symbol.line_start,
                            symbol.line_end
                        ],
                    )?;

                    tx.execute(
                        "INSERT INTO symbols_fts (fqname, signature, doc, path)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![symbol.fqname, symbol.signature, symbol.doc, path],
                    )?;
                }

                for reference in &found.refs {
                    tx.execute(
                        "INSERT INTO refs (from_symbol, to_name, path, line)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![reference.from_symbol, reference.to_name, path, reference.line],
                    )?;
                }
            }
        }

        tx.commit()?;
        Ok(Refresh::Indexed)
    }

    pub fn forget(&mut self, path: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM symbols_fts WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM refs WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        tx.commit()?;
        Ok(())
    }

    pub fn lookup(&self, name: &str, limit: usize) -> Result<Vec<SymbolRecord>> {
        let mut hits = self.query_symbols(
            "SELECT fqname, path, kind, signature, doc, line_start, line_end
             FROM symbols WHERE fqname = ?1 LIMIT ?2",
            params![name, limit as i64],
        )?;

        if hits.len() < limit {
            let suffix = format!("%.{name}");
            let more = self.query_symbols(
                "SELECT fqname, path, kind, signature, doc, line_start, line_end
                 FROM symbols WHERE fqname LIKE ?1 LIMIT ?2",
                params![suffix, limit as i64],
            )?;
            merge(&mut hits, more, limit);
        }

        if hits.is_empty() && !name.contains('.') {
            if let Some(query) = fts_query(name) {
                let more = self.query_symbols(
                    "SELECT s.fqname, s.path, s.kind, s.signature, s.doc, s.line_start, s.line_end
                     FROM symbols_fts f
                     JOIN symbols s ON s.fqname = f.fqname AND s.path = f.path
                     WHERE symbols_fts MATCH ?1
                     ORDER BY rank LIMIT ?2",
                    params![query, limit as i64],
                )?;
                merge(&mut hits, more, limit);
            }
        }

        Ok(hits)
    }

    pub fn slice(&self, fqname: &str, path: &str) -> Result<Option<Slice>> {
        let found = self.query_symbols(
            "SELECT fqname, path, kind, signature, doc, line_start, line_end
             FROM symbols WHERE fqname = ?1 AND path = ?2",
            params![fqname, path],
        )?;

        let Some(symbol) = found.into_iter().next() else {
            return Ok(None);
        };

        let calls = self.query_names(
            "SELECT DISTINCT to_name FROM refs WHERE from_symbol = ?1 ORDER BY to_name",
            params![fqname],
        )?;

        let leaf = fqname.rsplit('.').next().unwrap_or(fqname);
        let called_by = self.query_names(
            "SELECT DISTINCT from_symbol FROM refs WHERE to_name = ?1 ORDER BY from_symbol",
            params![leaf],
        )?;

        Ok(Some(Slice { symbol, calls, called_by }))
    }

    pub fn file(&self, path: &str) -> Result<Option<FileRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, lang, blake3, size, mtime, is_binary FROM files WHERE path = ?1",
        )?;
        let mut rows = stmt.query(params![path])?;
        match rows.next()? {
            Some(row) => Ok(Some(FileRow {
                path: row.get(0)?,
                lang: row.get(1)?,
                blake3: row.get(2)?,
                size: row.get::<_, i64>(3)? as u64,
                mtime: row.get(4)?,
                is_binary: row.get::<_, i64>(5)? != 0,
            })),
            None => Ok(None),
        }
    }

    pub fn count(&self, table: &str) -> Result<usize> {
        let sql = match table {
            "files" => "SELECT COUNT(*) FROM files",
            "symbols" => "SELECT COUNT(*) FROM symbols",
            "refs" => "SELECT COUNT(*) FROM refs",
            _ => return Ok(0),
        };
        let n: i64 = self.conn.query_row(sql, [], |r| r.get(0))?;
        Ok(n as usize)
    }

    fn known_hash(&self, path: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT blake3 FROM files WHERE path = ?1")?;
        let mut rows = stmt.query(params![path])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    fn query_symbols(
        &self,
        sql: &str,
        args: impl rusqlite::Params,
    ) -> Result<Vec<SymbolRecord>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(args, |row| {
            Ok(SymbolRecord {
                fqname: row.get(0)?,
                path: row.get(1)?,
                kind: row.get(2)?,
                signature: row.get(3)?,
                doc: row.get(4)?,
                line_start: row.get::<_, i64>(5)? as u32,
                line_end: row.get::<_, i64>(6)? as u32,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn query_names(&self, sql: &str, args: impl rusqlite::Params) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(args, |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn merge(hits: &mut Vec<SymbolRecord>, more: Vec<SymbolRecord>, limit: usize) {
    for candidate in more {
        if hits.len() >= limit {
            return;
        }
        let duplicate = hits
            .iter()
            .any(|h| h.fqname == candidate.fqname && h.path == candidate.path);
        if !duplicate {
            hits.push(candidate);
        }
    }
}

fn fts_query(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { ' ' })
        .collect();
    let terms: Vec<&str> = cleaned.split_whitespace().collect();
    if terms.is_empty() {
        return None;
    }
    Some(terms.iter().map(|t| format!("\"{t}\"*")).collect::<Vec<_>>().join(" OR "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &str = "class_name Player\n\nfunc take_damage(n: int) -> void:\n\thurt(n)\n";
    const ENEMY: &str = "class_name Enemy\n\nfunc attack() -> void:\n\ttake_damage(1)\n";

    fn seeded() -> Index {
        let mut index = Index::open_in_memory().unwrap();
        index.index_file("scripts/player.gd", PLAYER.as_bytes(), "t0").unwrap();
        index.index_file("scripts/enemy.gd", ENEMY.as_bytes(), "t0").unwrap();
        index
    }

    #[test]
    fn an_unchanged_file_is_not_reparsed() {
        let mut index = Index::open_in_memory().unwrap();
        assert_eq!(
            index.index_file("a.gd", PLAYER.as_bytes(), "t0").unwrap(),
            Refresh::Indexed
        );
        assert_eq!(
            index.index_file("a.gd", PLAYER.as_bytes(), "t1").unwrap(),
            Refresh::Unchanged
        );
    }

    #[test]
    fn a_changed_file_replaces_its_own_symbols_and_leaves_others_alone() {
        let mut index = seeded();
        index
            .index_file("scripts/player.gd", b"class_name Player\n\nfunc heal():\n\tpass\n", "t1")
            .unwrap();

        assert!(index.lookup("Player.take_damage", 5).unwrap().is_empty());
        assert_eq!(index.lookup("Player.heal", 5).unwrap().len(), 1);
        assert_eq!(index.lookup("Enemy.attack", 5).unwrap().len(), 1);
    }

    #[test]
    fn a_bare_leaf_name_finds_the_qualified_symbol() {
        let index = seeded();
        let hits = index.lookup("take_damage", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fqname, "Player.take_damage");
        assert_eq!(hits[0].path, "scripts/player.gd");
    }

    #[test]
    fn a_slice_carries_one_hop_in_both_directions() {
        let index = seeded();
        let slice = index.slice("Player.take_damage", "scripts/player.gd").unwrap().unwrap();
        assert_eq!(slice.calls, vec!["hurt"]);
        assert_eq!(slice.called_by, vec!["Enemy.attack"]);
    }

    #[test]
    fn a_slice_for_an_unknown_symbol_is_none_rather_than_an_error() {
        let index = seeded();
        assert!(index.slice("Nobody.method", "scripts/player.gd").unwrap().is_none());
    }

    #[test]
    fn forgetting_a_file_removes_its_symbols_refs_and_row() {
        let mut index = seeded();
        index.forget("scripts/enemy.gd").unwrap();

        assert!(index.file("scripts/enemy.gd").unwrap().is_none());
        assert!(index.lookup("Enemy.attack", 5).unwrap().is_empty());

        let slice = index.slice("Player.take_damage", "scripts/player.gd").unwrap().unwrap();
        assert!(slice.called_by.is_empty());
    }

    #[test]
    fn a_binary_file_is_recorded_but_never_parsed() {
        let mut index = Index::open_in_memory().unwrap();
        index.index_file("Content/Main.umap", &[0u8, 159, 146, 150], "t0").unwrap();

        let row = index.file("Content/Main.umap").unwrap().unwrap();
        assert!(row.is_binary);
        assert_eq!(row.lang, None);
        assert_eq!(index.count("symbols").unwrap(), 0);
    }

    #[test]
    fn a_cpp_file_is_tracked_as_a_file_even_without_an_extractor() {
        let mut index = Index::open_in_memory().unwrap();
        index.index_file("Source/Pawn.cpp", b"void f() {}", "t0").unwrap();

        let row = index.file("Source/Pawn.cpp").unwrap().unwrap();
        assert_eq!(row.lang.as_deref(), Some("cpp"));
        assert_eq!(index.count("symbols").unwrap(), 0);
    }

    #[test]
    fn the_same_fqname_in_two_files_does_not_overwrite_either() {
        let mut index = Index::open_in_memory().unwrap();
        let body = "func run():\n\tpass\n";
        index.index_file("a/util.gd", body.as_bytes(), "t0").unwrap();
        index.index_file("b/util.gd", body.as_bytes(), "t0").unwrap();

        let hits = index.lookup("util.run", 5).unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn a_csharp_and_a_gdscript_file_share_one_index() {
        let mut index = Index::open_in_memory().unwrap();
        index.index_file("scripts/player.gd", PLAYER.as_bytes(), "t0").unwrap();
        index
            .index_file("Assets/Mover.cs", b"class Mover { void Update() {} }", "t0")
            .unwrap();

        assert_eq!(index.lookup("Mover.Update", 5).unwrap().len(), 1);
        assert_eq!(index.lookup("Player.take_damage", 5).unwrap().len(), 1);
    }

    #[test]
    fn full_text_search_reaches_a_symbol_through_its_doc() {
        let mut index = Index::open_in_memory().unwrap();
        let src = "## restores hitpoints to the pawn\nfunc mend():\n\tpass\n";
        index.index_file("scripts/care.gd", src.as_bytes(), "t0").unwrap();

        let hits = index.lookup("hitpoints", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fqname, "care.mend");
    }

    #[test]
    fn an_exact_hit_is_never_diluted_by_fuzzy_matches() {
        let index = seeded();
        let hits = index.lookup("Player.take_damage", 5).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn a_qualified_name_that_is_gone_returns_nothing_rather_than_its_neighbours() {
        let mut index = seeded();
        index
            .index_file("scripts/player.gd", b"class_name Player\n\nfunc heal():\n\tpass\n", "t1")
            .unwrap();
        assert!(index.lookup("Player.take_damage", 5).unwrap().is_empty());
    }

    #[test]
    fn a_query_of_only_punctuation_returns_nothing_instead_of_an_fts_syntax_error() {
        let index = seeded();
        assert!(index.lookup("()", 5).unwrap().is_empty());
        assert!(index.lookup("\"", 5).unwrap().is_empty());
    }

    #[test]
    fn reindexing_does_not_leave_stale_rows_in_the_fts_shadow() {
        let mut index = Index::open_in_memory().unwrap();
        index
            .index_file("s.gd", b"## alpha marker\nfunc a():\n\tpass\n", "t0")
            .unwrap();
        index
            .index_file("s.gd", b"## beta marker\nfunc a():\n\tpass\n", "t1")
            .unwrap();

        assert!(index.lookup("alpha", 5).unwrap().is_empty());
        assert_eq!(index.lookup("beta", 5).unwrap().len(), 1);
    }
}
