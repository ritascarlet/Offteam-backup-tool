use crate::config::{BackupFrequency, Config};
use std::fs;
use std::process::Command;

pub struct SystemdService;

impl SystemdService {
    pub fn create(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
        let service_content = format!(
            r#"[Unit]
Description=OfficialVPN Backup Tool
After=network.target

[Service]
Type=simple
ExecStart={} --daemon
Restart=always
User={}

[Install]
WantedBy=multi-user.target
"#,
            std::env::current_exe()?.display(),
            std::env::var("USER").unwrap_or_else(|_| "root".to_string())
        );

        // Формируем расписание в зависимости от выбранной периодичности
        let calendar = match config
            .backup_frequency
            .as_ref()
            .unwrap_or(&BackupFrequency::Daily)
        {
            BackupFrequency::Daily => "*-*-*",
            BackupFrequency::Weekly => "Mon *-*-*", // Каждый понедельник
            BackupFrequency::Monthly => "*-*-1",    // Первый день каждого месяца
        };

        let timer_content = format!(
            r#"[Unit]
Description=OfficialVPN Backup Tool Timer

[Timer]
OnCalendar={} {}:00
Persistent=true

[Install]
WantedBy=timers.target
"#,
            calendar,
            config.backup_time.as_ref().unwrap_or(&"02:00".to_string())
        );

        if !Self::is_root() {
            return Err("Требуются права root для установки systemd сервиса".into());
        }

        fs::write("/etc/systemd/system/obt.service", service_content)?;
        fs::write("/etc/systemd/system/obt.timer", timer_content)?;

        Self::run_systemctl(&["daemon-reload"])?;
        Self::run_systemctl(&["enable", "obt.timer"])?;
        Self::run_systemctl(&["start", "obt.timer"])?;

        Ok(())
    }

    fn is_root() -> bool {
        nix::unistd::geteuid().is_root()
    }

    fn run_systemctl(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        let output = Command::new("systemctl").args(args).output()?;

        if !output.status.success() {
            return Err(format!(
                "Ошибка выполнения systemctl: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        Ok(())
    }
}
