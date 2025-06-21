pub mod uuid_wrapper {
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::{Timestamp, Uuid};

    pub fn _create_new_uuid() -> Uuid {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time ??")
            .as_secs();
        Uuid::new_v7(Timestamp::from_unix_time(current_time, 0, 0, 0))
    }

    pub fn _parse_uuid(uid: &str) -> Uuid {
        Uuid::parse_str(uid).unwrap()
    }

    #[test]
    fn test_create() {
        let _ = _create_new_uuid();
        assert!(true);
    }

    #[test]
    fn test_parse() {
        let uid_str = "01936f55-8d50-759e-acd1-104ad7953cf5";
        let uid = _parse_uuid(uid_str);
        assert!(uid_str == uid.to_string());
    }
}
