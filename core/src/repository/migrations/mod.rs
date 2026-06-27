use include_dir::{Dir, include_dir};
use lazy_static::lazy_static;
use rusqlite_migration::{M, Migrations};

static SQL_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/repository/migrations/sql");

lazy_static! {
    pub static ref MIGRATIONS: Migrations<'static> = {
        let mut files: Vec<_> = SQL_DIR.files().collect();
        files.sort_by_key(|f| f.path().to_path_buf());

        let migrations = files
            .into_iter()
            .filter(|f| f.path().extension().is_some_and(|ext| ext == "sql"))
            .map(|f| {
                M::up(
                    f.contents_utf8()
                        .unwrap_or_else(|| panic!("migration {:?} is not valid UTF-8", f.path())),
                )
            })
            .collect();

        Migrations::new(migrations)
    };
}
