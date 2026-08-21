use std::{
    fs::{self, File},
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client, multipart};
use tempfile::NamedTempFile;
use walkdir::WalkDir;
use zip::{
    CompressionMethod, ZipArchive, ZipWriter,
    write::{ExtendedFileOptions, FileOptions},
};

use crate::{config::ResolvedConfig, service::endpoint_url};

pub fn generate(
    input: &Path,
    endpoint_path: &str,
    scope: Option<&str>,
    config: &ResolvedConfig,
) -> Result<()> {
    if !input.exists() {
        bail!("input does not exist: {}", input.display());
    }

    fs::create_dir_all(&config.build_dir)
        .with_context(|| format!("failed to create {}", config.build_dir.display()))?;

    let upload_path = prepare_upload(input)?;
    println!("model input: {}", input.display());
    let zip_bytes = request_generation(&upload_path, endpoint_path, scope, config)?;
    let archive_path = config.build_dir.join("domain.zip");
    let error_file = config.build_dir.join("error.txt");
    if error_file.exists() {
        let _ = fs::remove_file(&error_file);
    }

    fs::write(&archive_path, &zip_bytes)
        .with_context(|| format!("failed to write {}", archive_path.display()))?;

    extract_zip(&zip_bytes, &config.build_dir)?;

    let error_file = config.build_dir.join("error.txt");
    if error_file.exists() {
        let content = fs::read_to_string(&error_file)
            .with_context(|| format!("failed to read {}", error_file.display()))?;
        bail!(content.trim().to_string());
    }

    println!("generated output in {}", config.build_dir.display());
    println!("archive saved to {}", archive_path.display());
    Ok(())
}

pub(crate) fn prepare_upload(input: &Path) -> Result<PathBuf> {
    if input.is_file() {
        return Ok(input.to_path_buf());
    }

    if input.is_dir() && !input.join("main.xml").exists() {
        let mut model_files = Vec::new();
        for entry in fs::read_dir(input).context("failed to read input directory")? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file()
                && let Some(ext) = path.extension()
                && (ext == "xml" || ext == "ksml")
            {
                model_files.push(path);
            }
        }

        if model_files.len() == 1 {
            println!(
                "note: uploading {} directly since no main.xml was found in directory",
                model_files[0].display()
            );
            return Ok(model_files[0].clone());
        } else if model_files.is_empty() {
            bail!(
                "no model files (.xml or .ksml) found in directory {}",
                input.display()
            );
        } else {
            bail!(
                "multiple model files found in {} but no main.xml. Please rename your entry point to main.xml",
                input.display()
            );
        }
    }

    let mut temp = NamedTempFile::new().context("failed to create temp zip file")?;
    zip_directory(input, temp.as_file_mut())?;
    Ok(temp.into_temp_path().keep()?)
}

fn request_generation(
    upload_path: &Path,
    endpoint_path: &str,
    scope: Option<&str>,
    config: &ResolvedConfig,
) -> Result<Vec<u8>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds))
        .build()
        .context("failed to build HTTP client")?;

    let upload_name = upload_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model.zip")
        .to_string();

    let file_part = multipart::Part::bytes(
        fs::read(upload_path)
            .with_context(|| format!("failed to read {}", upload_path.display()))?,
    )
    .file_name(upload_name);

    let mut form = multipart::Form::new().part("file", file_part);

    if let Some(scope) = scope {
        form = form.text("scope", scope.to_string());
    }

    let request_url = endpoint_url(&config.endpoint_prefix, endpoint_path);
    println!("using {}", request_url);
    let response = client
        .post(&request_url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .multipart(form)
        .send()
        .with_context(|| format!("request failed: {}", request_url))?;

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let is_zip = content_type.contains("zip") || content_type.contains("octet-stream");

    if !is_zip {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        if status.is_success() {
            println!("{}", body.trim());
            if body.contains("## ❌ Errors") {
                std::process::exit(1);
            }
            std::process::exit(0);
        } else {
            eprintln!("{}", body.trim());
            std::process::exit(1);
        }
    }

    if !response.status().is_success() {
        let body = response.text().unwrap_or_default();
        eprintln!("{}", body.trim());
        std::process::exit(1);
    }

    Ok(response.bytes()?.to_vec())
}

fn extract_zip(zip_bytes: &[u8], output_dir: &Path) -> Result<()> {
    let reader = Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(reader).context("response is not a valid zip archive")?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let enclosed = match entry.enclosed_name() {
            Some(path) => path.to_owned(),
            None => continue,
        };
        let output_path = output_dir.join(enclosed);

        if entry.name().ends_with('/') {
            fs::create_dir_all(&output_path)
                .with_context(|| format!("failed to create {}", output_path.display()))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut file = File::create(&output_path)
            .with_context(|| format!("failed to create {}", output_path.display()))?;
        io::copy(&mut entry, &mut file)
            .with_context(|| format!("failed to extract {}", output_path.display()))?;
    }

    Ok(())
}

fn zip_directory(directory: &Path, writer: &mut File) -> Result<()> {
    let mut zip = ZipWriter::new(writer);
    let options: FileOptions<'_, ExtendedFileOptions> =
        FileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut allowed_files = Vec::new();
    let main_xml = directory.join("main.xml");
    if main_xml.exists() {
        let mut queue = vec!["main.xml".to_string()];
        while let Some(file_name) = queue.pop() {
            if allowed_files.contains(&file_name) {
                continue;
            }
            allowed_files.push(file_name.clone());

            let file_path = directory.join(&file_name);
            if let Ok(content) = fs::read_to_string(&file_path) {
                for line in content.lines() {
                    if let Some(start) = line.find("<_include file=\"") {
                        let rest = &line[start + 16..];
                        if let Some(end) = rest.find("\"") {
                            let included = rest[..end].to_string();
                            if !allowed_files.contains(&included) && !queue.contains(&included) {
                                queue.push(included);
                            }
                        }
                    }
                }
            }
        }
    }

    for entry in WalkDir::new(directory) {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .strip_prefix(directory)
            .with_context(|| format!("failed to relativize {}", path.display()))?;

        if name.as_os_str().is_empty() {
            continue;
        }

        let name_string = name.to_string_lossy().replace('\\', "/");

        // Filter logic
        if main_xml.exists() {
            if entry.file_type().is_file() && !allowed_files.contains(&name_string) {
                continue;
            }
        } else {
            // Fallback: only include model files
            if entry.file_type().is_file() {
                let lower = name_string.to_lowercase();
                if !lower.ends_with(".xml")
                    && !lower.ends_with(".ksml")
                    && !lower.ends_with(".yml")
                    && !lower.ends_with(".yaml")
                {
                    continue;
                }
            }
        }

        if entry.file_type().is_dir() {
            zip.add_directory(format!("{name_string}/"), options.clone())?;
            continue;
        }

        zip.start_file(&name_string, options.clone())?;
        let mut file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        zip.write_all(&buffer)?;
    }

    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prepare_upload_returns_original_file_for_file_input() {
        let temp = tempdir().unwrap();
        let input = temp.path().join("model.yml");
        fs::write(&input, "name: demo").unwrap();

        let upload = prepare_upload(&input).unwrap();

        assert_eq!(upload, input);
    }

    #[test]
    fn prepare_upload_zips_directory_contents() {
        let temp = tempdir().unwrap();
        let input_dir = temp.path().join("model");
        fs::create_dir_all(input_dir.join("nested")).unwrap();
        fs::write(
            input_dir.join("main.xml"),
            "<_include file=\"root.xml\" />\n<_include file=\"nested/child.xml\" />",
        )
        .unwrap();
        fs::write(input_dir.join("root.xml"), "root").unwrap();
        fs::write(input_dir.join("nested").join("child.xml"), "child").unwrap();

        let upload = prepare_upload(&input_dir).unwrap();
        let zip_bytes = fs::read(upload).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(zip_bytes)).unwrap();

        let mut root = archive.by_name("root.xml").unwrap();
        let mut root_content = String::new();
        root.read_to_string(&mut root_content).unwrap();
        drop(root);

        let mut child = archive.by_name("nested/child.xml").unwrap();
        let mut child_content = String::new();
        child.read_to_string(&mut child_content).unwrap();

        assert_eq!(root_content, "root");
        assert_eq!(child_content, "child");
    }

    #[test]
    fn prepare_upload_zips_recursively_included_files() {
        let temp = tempdir().unwrap();
        let input_dir = temp.path().join("model");
        fs::create_dir_all(input_dir.join("nested")).unwrap();
        fs::write(input_dir.join("main.xml"), "<_include file=\"root.xml\" />").unwrap();
        fs::write(
            input_dir.join("root.xml"),
            "<_include file=\"nested/child.xml\" />\nroot",
        )
        .unwrap();
        fs::write(input_dir.join("nested").join("child.xml"), "child").unwrap();
        fs::write(input_dir.join("garbage.xml"), "garbage").unwrap();

        let upload = prepare_upload(&input_dir).unwrap();
        let zip_bytes = fs::read(upload).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(zip_bytes)).unwrap();

        // main.xml, root.xml, nested/child.xml should exist
        assert!(archive.by_name("main.xml").is_ok());
        assert!(archive.by_name("root.xml").is_ok());
        assert!(archive.by_name("nested/child.xml").is_ok());

        // garbage.xml should be filtered out
        assert!(archive.by_name("garbage.xml").is_err());
    }

    #[test]
    fn extract_zip_writes_files_to_output_directory() {
        let temp = tempdir().unwrap();
        let zip_path = temp.path().join("archive.zip");
        let mut file = File::create(&zip_path).unwrap();
        zip_directory(create_fixture_tree(temp.path()).as_path(), &mut file).unwrap();
        drop(file);

        let output_dir = temp.path().join("out");
        let zip_bytes = fs::read(zip_path).unwrap();
        extract_zip(&zip_bytes, &output_dir).unwrap();

        assert_eq!(
            fs::read_to_string(output_dir.join("root.xml")).unwrap(),
            "root"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("nested").join("child.xml")).unwrap(),
            "child"
        );
    }

    fn create_fixture_tree(base: &Path) -> PathBuf {
        let fixture = base.join("fixture");
        fs::create_dir_all(fixture.join("nested")).unwrap();
        fs::write(fixture.join("root.xml"), "root").unwrap();
        fs::write(fixture.join("nested").join("child.xml"), "child").unwrap();
        fixture
    }
}
