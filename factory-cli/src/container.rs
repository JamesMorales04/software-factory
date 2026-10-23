use crate::config::{CONTAINER_BIND, CONTAINER_DATABASE, CORE_CONTAINER_PORT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunSpec<'a> {
    pub(crate) name: &'a str,
    pub(crate) image: &'a str,
    pub(crate) volume: &'a str,
    pub(crate) env_file: &'a str,
    pub(crate) host_port: u16,
}

pub(crate) fn run_args(spec: &RunSpec<'_>) -> Vec<String> {
    vec![
        "run".to_owned(),
        "--pull=never".to_owned(),
        "--detach".to_owned(),
        "--init".to_owned(),
        "--name".to_owned(),
        spec.name.to_owned(),
        "--publish".to_owned(),
        format!("127.0.0.1:{}:{CORE_CONTAINER_PORT}", spec.host_port),
        "--volume".to_owned(),
        format!("{}:/data", spec.volume),
        "--read-only".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--cap-drop=ALL".to_owned(),
        "--security-opt".to_owned(),
        "no-new-privileges".to_owned(),
        "--network=podman".to_owned(),
        "--env-file".to_owned(),
        spec.env_file.to_owned(),
        "--env".to_owned(),
        format!("FACTORY_BIND={CONTAINER_BIND}"),
        "--env".to_owned(),
        "FACTORY_CONTAINER_MODE=1".to_owned(),
        "--env".to_owned(),
        format!("FACTORY_DATABASE={CONTAINER_DATABASE}"),
        spec.image.to_owned(),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartAction {
    Create,
    ReuseRunning,
    Replace,
}

pub(crate) fn start_action(state: Option<&str>) -> StartAction {
    match state {
        Some("running") => StartAction::ReuseRunning,
        Some(_) => StartAction::Replace,
        None => StartAction::Create,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VolumeAction {
    Create,
    Reuse,
}

pub(crate) fn volume_action(exists: bool) -> VolumeAction {
    if exists {
        VolumeAction::Reuse
    } else {
        VolumeAction::Create
    }
}

#[cfg(test)]
mod tests {
    use super::{RunSpec, StartAction, VolumeAction, run_args, start_action, volume_action};

    fn spec() -> RunSpec<'static> {
        RunSpec {
            name: "factory-core",
            image: "localhost/software-factory-core:dev",
            volume: "factory-core-data",
            env_file: "/home/operator/.config/factory/core.env",
            host_port: 8765,
        }
    }

    #[test]
    fn builds_the_restricted_container_arguments() {
        assert_eq!(
            run_args(&spec()),
            vec![
                "run",
                "--pull=never",
                "--detach",
                "--init",
                "--name",
                "factory-core",
                "--publish",
                "127.0.0.1:8765:8765",
                "--volume",
                "factory-core-data:/data",
                "--read-only",
                "--tmpfs",
                "/tmp",
                "--cap-drop=ALL",
                "--security-opt",
                "no-new-privileges",
                "--network=podman",
                "--env-file",
                "/home/operator/.config/factory/core.env",
                "--env",
                "FACTORY_BIND=0.0.0.0:8765",
                "--env",
                "FACTORY_CONTAINER_MODE=1",
                "--env",
                "FACTORY_DATABASE=/data/factory.sqlite",
                "localhost/software-factory-core:dev",
            ]
        );
    }

    #[test]
    fn never_uses_host_networking_or_the_podman_socket() {
        let args = run_args(&spec());
        assert!(!args.iter().any(|argument| argument == "--network=host"));
        assert!(!args.iter().any(|argument| argument.contains("podman.sock")));
        assert!(!args.iter().any(|argument| argument == "--privileged"));
        assert!(
            !args
                .iter()
                .any(|argument| argument.contains("FACTORY_OPERATOR_TOKEN"))
        );
    }

    #[test]
    fn uses_the_configured_host_port_for_loopback_publication() {
        let mut custom = spec();
        custom.host_port = 9123;
        let args = run_args(&custom);
        let publish = args
            .windows(2)
            .find(|window| window[0] == "--publish")
            .map(|window| window[1].as_str());
        assert_eq!(publish, Some("127.0.0.1:9123:8765"));
    }

    #[test]
    fn start_is_idempotent_for_a_running_container() {
        assert_eq!(start_action(None), StartAction::Create);
        assert_eq!(start_action(Some("running")), StartAction::ReuseRunning);
        assert_eq!(start_action(Some("exited")), StartAction::Replace);
        assert_eq!(start_action(Some("created")), StartAction::Replace);
    }

    #[test]
    fn reuses_an_existing_data_volume() {
        assert_eq!(volume_action(true), VolumeAction::Reuse);
        assert_eq!(volume_action(false), VolumeAction::Create);
    }
}
