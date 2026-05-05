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
}