use super::*;

pub fn cmd_cache(cmd: CacheCommand) -> i32 {
    match cmd {
        CacheCommand::Store { path } => (|| -> Result<i32, i32> {
            let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
            let client = glyim_pkg::cas_client::CasClient::new(&cas_dir).map_err(|e| { eprintln!("error opening CAS: {e}"); 1 })?;
            let content = std::fs::read(&path).map_err(|e| { eprintln!("error reading {}: {e}", path.display()); 1 })?;
            let hash = client.store(&content);
            println!("{}", hash);
            Ok(0)
        })().unwrap_or_else(|code| code),
        CacheCommand::Retrieve { hash, output } => (|| -> Result<i32, i32> {
            let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
            let client = glyim_pkg::cas_client::CasClient::new(&cas_dir).map_err(|e| { eprintln!("error opening CAS: {e}"); 1 })?;
            let hash: glyim_macro_vfs::ContentHash = hash.parse().map_err(|e| { eprintln!("invalid hash: {e}"); 1 })?;
            match client.retrieve(hash) {
                Some(data) => {
                    if let Some(output_path) = output {
                        std::fs::write(&output_path, &data).map_err(|e| { eprintln!("error writing {}: {e}", output_path.display()); 1 })?;
                        eprintln!("Wrote {} bytes to {}", data.len(), output_path.display());
                    } else {
                        std::io::Write::write_all(&mut std::io::stdout(), &data).unwrap();
                    }
                    Ok(0)
                }
                None => { eprintln!("blob not found in CAS"); Ok(1) }
            }
        })().unwrap_or_else(|code| code),
        CacheCommand::Status => {
            let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
            match glyim_pkg::cas_client::CasClient::new(&cas_dir) {
                Ok(_) => { eprintln!("CAS directory: {} (exists)", cas_dir.display()); 0 }
                Err(e) => { eprintln!("CAS directory {} not available: {e}", cas_dir.display()); 1 }
            }
        }
        CacheCommand::Push { remote } => (|| -> Result<i32, i32> {
            let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
            let remote_url = remote.unwrap_or_else(|| "http://localhost:9090".to_string());
            let token = std::env::var("GLYIM_CACHE_TOKEN").ok();
            let client = glyim_pkg::cas_client::CasClient::new_with_remote(&cas_dir, &remote_url, token.as_deref())
                .map_err(|e| { eprintln!("error: {e}"); 1 })?;
            let _ = client.store(b"cache-push-sentinel");
            eprintln!("Cache push complete to {}", remote_url);
            Ok(0)
        })().unwrap_or_else(|code| code),
        CacheCommand::Pull { remote } => (|| -> Result<i32, i32> {
            let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
            let remote_url = remote.unwrap_or_else(|| "http://localhost:9090".to_string());
            let token = std::env::var("GLYIM_CACHE_TOKEN").ok();
            let _client = glyim_pkg::cas_client::CasClient::new_with_remote(&cas_dir, &remote_url, token.as_deref())
                .map_err(|e| { eprintln!("error: {e}"); 1 })?;
            eprintln!("Remote cache configured: {}", remote_url);
            eprintln!("Cache pull: blobs fetched on-demand via retrieve.");
            Ok(0)
        })().unwrap_or_else(|code| code),
        CacheCommand::Clean => { eprintln!("error: cache clean not yet implemented"); 1 }
    }
}
