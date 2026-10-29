use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

use crumbs_database::{DatabaseConfig, DatabasePath};
use crumbs_server::HttpConfig;
use tempfile::NamedTempFile;

use super::AppConfig;

#[test]
fn loads_sample_configuration() -> Result<(), Box<dyn Error>> {
    let file = temporary_config(include_str!("../../resources/application-sample.toml"))?;
    let parent = file
        .path()
        .parent()
        .ok_or_else(|| io::Error::other("temporary config has no parent"))?;

    let config = AppConfig::from_file(file.path())?;

    assert_eq!(
        config,
        AppConfig {
            database: DatabaseConfig {
                path: parent.join("data/crumbs.db")
            },
            http: HttpConfig { port: 8080 },
        }
    );
    Ok(())
}

#[test]
fn loads_valid_nested_configuration() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\nport = 49152\n\n[database]\npath = \"data/crumbs.db\"\n")?;
    let parent = file
        .path()
        .parent()
        .ok_or_else(|| io::Error::other("temporary config has no parent"))?;

    let config = AppConfig::from_file(file.path())?;

    assert_eq!(
        config,
        AppConfig {
            database: DatabaseConfig {
                path: parent.join("data/crumbs.db")
            },
            http: HttpConfig { port: 49152 },
        }
    );
    Ok(())
}

#[test]
fn resolves_relative_database_path_against_config_directory() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let config_directory = directory.path().join("configuration");
    fs::create_dir(&config_directory)?;
    let config_path = config_directory.join("application.toml");
    fs::write(
        &config_path,
        "[http]\nport = 8080\n\n[database]\npath = \"state/crumbs.db\"\n",
    )?;

    let config = AppConfig::from_file(&config_path)?;

    assert_eq!(
        config.database.path,
        config_directory.join("state").join("crumbs.db")
    );
    Ok(())
}

#[test]
fn preserves_absolute_database_path() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("elsewhere").join("crumbs.db");
    let file = temporary_config(&format!(
        "[http]\nport = 8080\n\n[database]\npath = \"{}\"\n",
        database_path.display()
    ))?;

    let config = AppConfig::from_file(file.path())?;

    assert_eq!(config.database.path, database_path);
    Ok(())
}

#[test]
fn rejects_missing_configuration_file() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("missing.toml");

    assert!(AppConfig::from_file(path).is_err());
    Ok(())
}

#[test]
fn rejects_malformed_toml() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http\nport = 8080\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_missing_database_section() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\nport = 8080\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_missing_http_section() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[database]\npath = \"data/crumbs.db\"\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_missing_port() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\n\n[database]\npath = \"data/crumbs.db\"\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_wrong_port_type() -> Result<(), Box<dyn Error>> {
    let file =
        temporary_config("[http]\nport = [8080]\n\n[database]\npath = \"data/crumbs.db\"\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_out_of_range_port() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\nport = 70000\n\n[database]\npath = \"data/crumbs.db\"\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_missing_database_path() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\nport = 8080\n\n[database]\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_wrong_database_path_type() -> Result<(), Box<dyn Error>> {
    let file =
        temporary_config("[http]\nport = 8080\n\n[database]\npath = [\"data/crumbs.db\"]\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_unknown_root_section() -> Result<(), Box<dyn Error>> {
    let file = temporary_config(
        "[http]\nport = 8080\n\n[database]\npath = \"data/crumbs.db\"\n\n[server]\nport = 8080\n",
    )?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_unknown_http_key() -> Result<(), Box<dyn Error>> {
    let file = temporary_config(
        "[http]\nport = 8080\nhost = \"127.0.0.1\"\n\n[database]\npath = \"data/crumbs.db\"\n",
    )?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_unknown_database_key() -> Result<(), Box<dyn Error>> {
    let file = temporary_config(
        "[http]\nport = 8080\n\n[database]\npath = \"data/crumbs.db\"\nmode = \"wal\"\n",
    )?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn database_path_alias_is_a_path() {
    fn require_path<T: AsRef<Path>>() {}
    require_path::<DatabasePath>();
}

fn temporary_config(contents: &str) -> Result<NamedTempFile, Box<dyn Error>> {
    let file = NamedTempFile::new()?;
    fs::write(file.path(), contents)?;
    Ok(file)
}
