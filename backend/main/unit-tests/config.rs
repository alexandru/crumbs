use std::error::Error;
use std::fs;

use crumbs_server::HttpConfig;
use tempfile::NamedTempFile;

use super::AppConfig;

#[test]
fn loads_sample_configuration() -> Result<(), Box<dyn Error>> {
    let file = temporary_config(include_str!("../../resources/application-sample.toml"))?;

    let config = AppConfig::from_file(file.path())?;

    assert_eq!(
        config,
        AppConfig {
            http: HttpConfig { port: 8080 }
        }
    );
    Ok(())
}

#[test]
fn loads_valid_nested_configuration() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\nport = 49152\n")?;

    let config = AppConfig::from_file(file.path())?;

    assert_eq!(
        config,
        AppConfig {
            http: HttpConfig { port: 49152 }
        }
    );
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
fn rejects_missing_http_section() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[server]\nport = 8080\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_missing_port() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_wrong_port_type() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\nport = [8080]\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_out_of_range_port() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\nport = 70000\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_unknown_root_section() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\nport = 8080\n\n[database]\npath = \"crumbs.db\"\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

#[test]
fn rejects_unknown_http_key() -> Result<(), Box<dyn Error>> {
    let file = temporary_config("[http]\nport = 8080\nhost = \"127.0.0.1\"\n")?;

    assert!(AppConfig::from_file(file.path()).is_err());
    Ok(())
}

fn temporary_config(contents: &str) -> Result<NamedTempFile, Box<dyn Error>> {
    let file = NamedTempFile::new()?;
    fs::write(file.path(), contents)?;
    Ok(file)
}
