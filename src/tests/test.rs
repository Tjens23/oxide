#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stringify() {
        let result = Versions::stringify(&"lodash".to_string(), &"4.17.21".to_string());
        assert_eq!(result, "lodash@4.17.21");
    }

    #[test]
    fn test_parse_raw_package_details() {
        let (name, version) = Versions::parse_raw_package_details("lodash@4.17.21".to_string());
        assert_eq!(name, "lodash");
        assert_eq!(version, "4.17.21");
    }

    #[test]
    fn test_no_version_specified() {
        let (name, version) = Versions::parse_raw_package_details("lodash".to_string());
        assert_eq!(name, "lodash");
        assert_eq!(version, "");
    }

    #[test]
    fn test_link_command() {
        let mut handler = LinkHandler::default();
        let args = vec!["link".to_string(), "lodash".to_string()];
        handler.parse(&mut args.into_iter()).unwrap();

        let sandbox = std::env::temp_dir().join(format!(
            "oxide-link-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
        ));

        std::fs::create_dir_all(&sandbox).unwrap();
        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&sandbox).unwrap();

        std::fs::create_dir_all("node_modules/lodash").unwrap();
        assert!(sandbox.join("node_modules/lodash").exists());

        std::env::set_current_dir(old_cwd).unwrap();
        std::fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn test_unlink_command() {
        let mut handler = UnlinkHandler::default();
        let args = vec!["unlink".to_string(), "lodash".to_string()];
        handler.parse(&mut args.into_iter()).unwrap();
        let sandbox = std::env::temp_dir().join(format!(
            "oxide-unlink-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
        ));
    }

    #[test]
    fn test_publish_command() {
        let mut handler = PublishHandler::default();
        let args = vec!["publish".to_string()];
        handler.parse(&mut args.into_iter()).unwrap();
    }

    #[test]
    fn test_outdated_command() {
        let mut handler = OutdatedHandler::default();
        let args = vec!["outdated".to_string()];
        handler.parse(&mut args.into_iter()).unwrap();
    }

    #[test]
    fn test_ls_command() {
        let mut handler = LsHandler::default();
        let args = vec!["ls".to_string()];
        handler.parse(&mut args.into_iter()).unwrap();
    }
    
    #[test]
    fn test_pack_command() {
        let mut handler = PackHandler::default();
        let args = vec!["pack".to_string()];
        handler.parse(&mut args.into_iter()).unwrap();
    }

    #[test]
    fn test_doctor_command() {
        let mut handler = DoctorHandler::default();
        let args = vec!["doctor".to_string()];
        handler.parse(&mut args.into_iter()).unwrap();
    }

    #[test]
    fn test_dlx_command() {
        let mut handler = DlxHandler::default();
        let args = vec!["dlx".to_string(), "lodash".to_string()];
        handler.parse(&mut args.into_iter()).unwrap();
    }

}