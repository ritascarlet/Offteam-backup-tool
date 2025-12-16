mod config;
mod systemd;

use chrono::{Datelike, Local, NaiveTime, Timelike, Utc};
use chrono_tz::Europe::Moscow;
use colored::*;
use config::{BackupFrequency, Config};
use log::{info, warn, error};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use std::fs;
use std::io::{self, Write};
use std::process::Command;
use systemd::SystemdService;

fn read_input(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn setup_gitea(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nНастройка Gitea");

    let full_repo_url = read_input(
        "Введите полный URL репозитория Gitea (например, backups.tgvpnbot.com/alex/backup): ",
    )?;
    let clean_url = full_repo_url.replace("https://", "");

    if let Some(last_slash_pos) = clean_url.rfind('/') {
        let (base_url, repo_path) = clean_url.split_at(last_slash_pos);
        let repo_path = repo_path.trim_start_matches('/');

        config.gitea_url = Some(base_url.to_string());
        config.gitea_repo = Some(repo_path.to_string());
    }

    config.gitea_username = Some(read_input("Введите имя пользователя Gitea: ")?);
    config.gitea_password = Some(read_input("Введите пароль пользователя Gitea: ")?);

    config.save()?;
    println!("{}", "Настройки Gitea успешно сохранены!".green());
    Ok(())
}

fn setup_backup_name(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nНастройка имени для бэкапов");
    let name = read_input("Введите имя для бэкапов (например, название сервера): ")?;
    config.backup_name = Some(name);
    config.save()?;
    println!("{}", "Имя бэкапа установлено!".green());
    Ok(())
}

fn restart_daemon() -> Result<(), Box<dyn std::error::Error>> {
    info!("Перезапуск демона для применения новых настроек времени...");
    
    std::process::Command::new("systemctl")
        .args(&["restart", "obt.service"])
        .output()?;
        
    std::process::Command::new("systemctl")
        .args(&["restart", "obt.timer"])
        .output()?;
        
    println!("{}", "✅ Демон перезапущен для применения нового времени".green());
    Ok(())
}

fn get_moscow_time() -> chrono::DateTime<chrono_tz::Tz> {
    Utc::now().with_timezone(&Moscow)
}

fn setup_backup_schedule(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nНастройка расписания бэкапов");
    println!("{}", "⏰ Время указывается по московскому времени (MSK)".yellow());

    println!("Выберите периодичность бэкапов:");
    println!("1. Ежедневно");
    println!("2. Еженедельно");
    println!("3. Ежемесячно");

    let frequency = match read_input("Выберите вариант (1-3): ")?.as_str() {
        "1" => BackupFrequency::Daily,
        "2" => BackupFrequency::Weekly,
        "3" => BackupFrequency::Monthly,
        _ => return Err("Неверный выбор".into()),
    };

    let moscow_time = get_moscow_time();
    println!("Текущее московское время: {}", moscow_time.format("%H:%M:%S"));

    let time = loop {
        let input = read_input("Введите время для бэкапа по МСК (ЧЧ:ММ): ")?;
        if let Ok(_) = NaiveTime::parse_from_str(&input, "%H:%M") {
            break input;
        }
        println!("Неверный формат времени. Попробуйте снова.");
    };

    config.backup_frequency = Some(frequency);
    config.backup_time = Some(time);
    config.save()?;

    SystemdService::create(config)?;

    // Автоматически перезапускаем демон
    if let Err(e) = restart_daemon() {
        warn!("Не удалось перезапустить демон: {}", e);
        println!("{}", "⚠️ Перезапустите демон вручную: sudo systemctl restart obt.service".yellow());
    }

    println!("{}", "Расписание бэкапов настроено (по московскому времени)!".green());
    Ok(())
}

fn manage_backup_paths(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        println!("\nТекущие пути для бэкапа:");
        if config.backup_paths.is_empty() {
            println!("Нет добавленных путей");
        } else {
            for (i, path) in config.backup_paths.iter().enumerate() {
                println!("{}. {}", i + 1, path);
            }
        }

        println!("\nДействия:");
        println!("1. Добавить новый путь");
        println!("2. Удалить все пути");
        println!("3. Вернуться в главное меню");

        match read_input("\nВыберите действие (1-3): ")?.as_str() {
            "1" => {
                let path = read_input(
                    "\nДобавьте директорию или файл для бэкапирования (укажите путь): ",
                )?;
                let path_obj = std::path::Path::new(&path);

                if path_obj.exists() {
                    if !config.backup_paths.contains(&path) {
                        config.backup_paths.push(path);
                        println!("{}", "Путь успешно добавлен!".green());
                    } else {
                        println!("{}", "Этот путь уже добавлен!".yellow());
                    }
                } else {
                    println!("{}", "Указанный путь не существует!".red());
                    if read_input("Создать директорию? (y/n): ")?.to_lowercase() == "y"
                    {
                        fs::create_dir_all(path_obj)?;
                        config.backup_paths.push(path);
                        println!("{}", "Директория создана и добавлена!".green());
                    }
                }
                config.save()?;
            }
            "2" => {
                if !config.backup_paths.is_empty() {
                    println!(
                        "{}",
                        "Внимание! Это действие удалит все пути для бэкапа!".red()
                    );
                    if read_input("Вы уверены? (y/n): ")?.to_lowercase() == "y" {
                        config.backup_paths.clear();
                        config.save()?;
                        println!("{}", "Все пути успешно удалены!".green());
                    }
                } else {
                    println!("{}", "Список путей уже пуст!".yellow());
                }
            }
            "3" => break,
            _ => println!("Неверный выбор, попробуйте снова"),
        }
    }
    Ok(())
}



fn execute_command_with_retry(cmd: &str, max_retries: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut last_error = None;
    info!("Выполнение команды: {}", cmd);
    
    for attempt in 1..=max_retries {
        match Command::new("sh").arg("-c").arg(cmd).output() {
            Ok(output) => {
                if output.status.success() || cmd.contains("git pull") {
                    let output_str = String::from_utf8_lossy(&output.stdout);
                    if !output_str.is_empty() {
                        info!("Вывод команды: {}", output_str);
                        println!("{}", output_str);
                    }
                    info!("Команда выполнена успешно: {}", cmd);
                    return Ok(());
                } else {
                    let error = String::from_utf8_lossy(&output.stderr);
                    let error_msg = format!("Ошибка при выполнении команды: {}", error);
                    warn!("Попытка {} из {} не удалась для команды '{}': {}", attempt, max_retries, cmd, error);
                    last_error = Some(error_msg);
                    if attempt < max_retries {
                        println!("Попытка {} не удалась, повтор через 5 сек...", attempt);
                        std::thread::sleep(std::time::Duration::from_secs(5));
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Ошибка при выполнении команды: {}", e);
                warn!("Попытка {} из {} не удалась для команды '{}': {}", attempt, max_retries, cmd, e);
                last_error = Some(error_msg);
                if attempt < max_retries {
                    println!("Попытка {} не удалась, повтор через 5 сек...", attempt);
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
            }
        }
    }
    
    let final_error = last_error.unwrap_or_else(|| "Неизвестная ошибка".to_string());
    error!("Все попытки исчерпаны для команды '{}': {}", cmd, final_error);
    Err(final_error.into())
}

fn create_gitignore(backup_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let gitignore_content = r#"# Временные файлы
*.tmp
*.temp
*.log
*.pid
*.swp
*.swo
*~

# Системные файлы
.DS_Store
Thumbs.db
desktop.ini

# Большие файлы (больше 100MB будут игнорироваться)
*.iso
*.img
*.dmg
*.vdi
*.vmdk

# Кэши
*.cache
cache/
.cache/
node_modules/
.npm/
.yarn/

# Личные данные
*.key
*.pem
*.p12
*.pfx
id_rsa
id_ecdsa
id_ed25519
"#;
    let gitignore_path = format!("{}/.gitignore", backup_dir);
    fs::write(gitignore_path, gitignore_content)?;
    Ok(())
}

fn perform_backup(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    if config.backup_paths.is_empty() {
        return Err("Нет путей для бэкапа! Сначала добавьте файлы/директории.".into());
    }

    info!("Начинаем выполнение бэкапа...");
    println!("🚀 Выполняется бэкап с tar.gz сжатием...");

    let repo_url = format!(
        "https://{}:{}@{}/{}.git",
        config
            .gitea_username
            .as_ref()
            .ok_or("Не настроен логин Gitea")?,
        utf8_percent_encode(
            config
                .gitea_password
                .as_ref()
                .ok_or("Не настроен пароль Gitea")?,
            NON_ALPHANUMERIC
        ),
        config.gitea_url.as_ref().ok_or("Не настроен URL Gitea")?,
        config
            .gitea_repo
            .as_ref()
            .ok_or("Не настроен репозиторий Gitea")?
    );

    let moscow_time = get_moscow_time();
    let backup_dir = format!("/tmp/backup_{}", moscow_time.format("%Y%m%d_%H%M%S"));
    fs::create_dir_all(&backup_dir)?;
    info!("Создана временная папка: {}", backup_dir);

    // Git конфигурации для стабильности
    let git_configs = vec![
        format!("cd {} && git init", backup_dir),
        format!("cd {} && git config user.name \"{}\"", backup_dir, config.gitea_username.as_ref().unwrap()),
        format!("cd {} && git config user.email \"{}@backup.local\"", backup_dir, config.gitea_username.as_ref().unwrap()),
        format!("cd {} && git config http.postBuffer 524288000", backup_dir), // 500MB buffer
        format!("cd {} && git config http.timeout 300", backup_dir), // 5 минут timeout
        format!("cd {} && git config core.compression 9", backup_dir), // Максимальное сжатие
        format!("cd {} && git config push.default simple", backup_dir),
        format!("cd {} && git config pull.rebase false", backup_dir),
        format!("cd {} && git remote add origin {}", backup_dir, repo_url),
    ];

    println!("⚙️ Настройка Git репозитория...");
    for cmd in git_configs {
        execute_command_with_retry(&cmd, 3)?;
    }

    // Проверяем существование удаленного репозитория и определяем ветку
    let default_branch = if execute_command_with_retry(&format!("cd {} && git ls-remote --heads origin main", backup_dir), 2).is_ok() {
        "main"
    } else {
        "master"
    };
    info!("Используем ветку: {}", default_branch);

    // Синхронизация с удаленным репозиторием
    let sync_commands = vec![
        format!("cd {} && git fetch origin {} || true", backup_dir, default_branch),
        format!("cd {} && (git checkout {} || git checkout -b {})", backup_dir, default_branch, default_branch),
        format!("cd {} && git pull origin {} --no-edit || true", backup_dir, default_branch),
    ];

    println!("🔄 Синхронизация с удаленным репозиторием...");
    for cmd in sync_commands {
        execute_command_with_retry(&cmd, 3)?;
    }

    // Создаем .gitignore только если его нет
    let gitignore_path = format!("{}/.gitignore", backup_dir);
    if !std::path::Path::new(&gitignore_path).exists() {
        create_gitignore(&backup_dir)?;
        info!("Создан .gitignore файл");
    }

    // Создаем папку для бэкапов
    let backup_folder_name = match &config.backup_name {
        Some(name) => format!("{}_{}", name, moscow_time.format("%Y%m%d_%H%M%S")),
        None => moscow_time.format("%Y%m%d_%H%M%S").to_string(),
    };
    let current_backup_dir = format!("{}/{}", backup_dir, backup_folder_name);
    fs::create_dir_all(&current_backup_dir)?;

    // Переменные для статистики
    let mut total_size = 0u64;
    let mut archive_info = Vec::new();

    // Создаем tar.gz архивы для каждого пути
    println!("📦 Создание tar.gz архивов...");
    for (index, path) in config.backup_paths.iter().enumerate() {
        let path_obj = std::path::Path::new(path);
        let archive_name = if path_obj.is_file() {
            format!("file_{}_{}.tar.gz", index + 1, path_obj.file_name().unwrap().to_string_lossy())
        } else {
            format!("dir_{}_{}.tar.gz", index + 1, path_obj.file_name().unwrap_or(std::ffi::OsStr::new("unknown")).to_string_lossy())
        };

        let archive_path = format!("{}/{}", current_backup_dir, archive_name);
        
        println!("📁 Архивирование: {} → {}", path, archive_name);

        // Создаем tar.gz архив
        let tar_command = if path_obj.is_file() {
            let parent_dir = path_obj.parent().unwrap_or(std::path::Path::new("/"));
            let filename = path_obj.file_name().unwrap().to_string_lossy();
            format!("tar -czf {} -C {} {}", archive_path, parent_dir.display(), filename)
        } else {
            format!("tar -czf {} -C {} .", archive_path, path)
        };

        match execute_command_with_retry(&tar_command, 3) {
            Ok(_) => {
                // Получаем размер архива
                if let Ok(metadata) = fs::metadata(&archive_path) {
                    let size = metadata.len();
                    total_size += size;
                    archive_info.push(format!("  📦 {} ({:.2} МБ)", archive_name, size as f64 / 1_048_576.0));
                    info!("Архив создан: {} (размер: {} байт)", archive_name, size);
                } else {
                    archive_info.push(format!("  📦 {} (размер неизвестен)", archive_name));
                }
            }
            Err(e) => {
                warn!("Не удалось создать архив напрямую: {}. Пробуем fallback...", e);
                
                // Fallback: копируем во временную папку, затем архивируем
                let temp_copy_dir = format!("/tmp/temp_copy_{}", index);
                fs::create_dir_all(&temp_copy_dir)?;
                
                let copy_cmd = if path_obj.is_file() {
                    format!("cp {} {}/", path, temp_copy_dir)
                } else {
                    format!("rsync -av --timeout=300 {}/ {}/", path, temp_copy_dir)
                };
                
                execute_command_with_retry(&copy_cmd, 3)?;
                
                let tar_fallback_cmd = format!("tar -czf {} -C {} .", archive_path, temp_copy_dir);
                execute_command_with_retry(&tar_fallback_cmd, 3)?;
                
                // Удаляем временную папку
                fs::remove_dir_all(&temp_copy_dir)?;
                
                if let Ok(metadata) = fs::metadata(&archive_path) {
                    let size = metadata.len();
                    total_size += size;
                    archive_info.push(format!("  📦 {} ({:.2} МБ)", archive_name, size as f64 / 1_048_576.0));
                    info!("Архив создан (fallback): {} (размер: {} байт)", archive_name, size);
                }
            }
        }
    }

    // Создаем файл с информацией о бэкапе
    let backup_info = format!(
        r#"🌍 OfficialVPN Backup Tool v0.1.3 - Информация о бэкапе

📅 Дата и время: {} MSK
🏷️  Имя бэкапа: {}
📊 Общий размер архивов: {:.2} МБ
📦 Количество архивов: {}

📋 Архивы:
{}

💾 Исходные пути:
{}

🔧 Технические детали:
- Формат: tar.gz (gzip сжатие)  
- Временная зона: Московское время (MSK)
- Git ветка: {}
- Кодировка: UTF-8

🌍 Сервер: {}
👤 Пользователь: {}
"#,
        moscow_time.format("%Y-%m-%d %H:%M:%S"),
        backup_folder_name,
        total_size as f64 / 1_048_576.0,
        archive_info.len(),
        archive_info.join("\n"),
        config.backup_paths.iter().map(|p| format!("  📂 {}", p)).collect::<Vec<_>>().join("\n"),
        default_branch,
        config.gitea_url.as_ref().unwrap_or(&"неизвестно".to_string()),
        config.gitea_username.as_ref().unwrap_or(&"неизвестно".to_string())
    );

    let info_path = format!("{}/backup_info.txt", current_backup_dir);
    fs::write(&info_path, backup_info)?;
    info!("Создан файл backup_info.txt");

    // Коммитим и пушим все изменения одним коммитом
    println!("🚀 Загрузка в репозиторий...");
    
    let final_commands = vec![
        format!("cd {} && git add .", backup_dir),
        format!("cd {} && git commit -m '🌍 Backup {} - {} архивов ({:.1} МБ) - MSK {}'", 
                backup_dir, 
                backup_folder_name, 
                archive_info.len(),
                total_size as f64 / 1_048_576.0,
                moscow_time.format("%Y-%m-%d %H:%M")
        ),
        format!("cd {} && git pull origin {} --no-edit", backup_dir, default_branch),
        format!("cd {} && git push origin {}", backup_dir, default_branch),
    ];

    for cmd in final_commands {
        execute_command_with_retry(&cmd, 3)?;
    }

    // Очистка
    fs::remove_dir_all(&backup_dir)?;
    info!("Временные файлы удалены");

    // Обновляем конфигурацию
    config.last_backup = Some(moscow_time.format("%Y-%m-%d %H:%M:%S MSK").to_string());
    config.save()?;

    println!("{}", "✅ Бэкап успешно выполнен!".green());
    println!("📊 Общий размер архивов: {:.2} МБ", total_size as f64 / 1_048_576.0);
    println!("📦 Создано архивов: {}", archive_info.len());
    info!("Бэкап завершен успешно. Общий размер: {} байт", total_size);

    Ok(())
}

fn run_daemon_mode(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    info!("Запуск демона с расписанием: {:?} (московское время)", config.backup_time);
    println!("Запуск в режиме демона...");
    println!("{}", "⏰ Работа по московскому времени (MSK)".yellow());

    let mut last_backup_day = 0;

    loop {
        // Используем московское время вместо локального
        let moscow_now = get_moscow_time();
        
        if let Some(backup_time) = &config.backup_time {
            if let Ok(target_time) = NaiveTime::parse_from_str(backup_time, "%H:%M") {
                let current_time = moscow_now.time();
                let current_day = moscow_now.ordinal();

                // Проверяем, что настало время бэкапа и мы еще не делали бэкап сегодня
                if current_time.hour() == target_time.hour()
                    && current_time.minute() == target_time.minute()
                    && current_day != last_backup_day
                {
                    info!("Настало время автоматического бэкапа (MSK): {}", moscow_now.format("%Y-%m-%d %H:%M:%S"));
                    
                    match perform_backup(config) {
                        Ok(_) => {
                            info!("Автоматический бэкап выполнен успешно");
                            last_backup_day = current_day;
                        }
                        Err(e) => {
                            error!("Ошибка при выполнении автоматического бэкапа: {}", e);
                            eprintln!("Ошибка при выполнении автоматического бэкапа: {}", e);
                        }
                    }
                    
                    // Ждем минуту, чтобы не запускать бэкап повторно в ту же минуту
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            } else {
                warn!("Неверный формат времени в конфигурации: {}", backup_time);
            }
        } else {
            warn!("Время бэкапа не настроено");
        }
        
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Инициализируем логгер
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();
    
    info!("Запуск OfficialVPN Backup Tool v{}", env!("CARGO_PKG_VERSION"));
    
    let mut config = Config::load()?;

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--daemon" {
        info!("Запуск в режиме демона");
        return run_daemon_mode(&mut config);
    }

    if config.gitea_repo.is_none() {
        println!("Добро пожаловать в OBT! Давайте настроим резервное копирование.");
        setup_gitea(&mut config)?;
        setup_backup_name(&mut config)?;
        setup_backup_schedule(&mut config)?;
        manage_backup_paths(&mut config)?;
    }

    loop {
        println!("\n{}", "OfficialVPN Backup Tools".green());
        let moscow_time = get_moscow_time();
        let local_time = Local::now();
        println!(
            "Локальное время: {} | Московское время: {}",
            local_time.format("%Y-%m-%d %H:%M:%S"),
            moscow_time.format("%Y-%m-%d %H:%M:%S MSK")
        );

        if let Some(last_backup) = &config.last_backup {
            println!("Последний бэкап: {}", last_backup.white().bold());
        }
        if let Some(name) = &config.backup_name {
            println!("Имя бэкапа: {}", name.white().bold());
        }
        if let Some(time) = &config.backup_time {
            println!("Время бэкапа: {}", time.white().bold());
        }

        println!("\nМеню:");
        println!("1. Сделать бэкап");
        println!("2. Добавить/изменить файлы для бэкапа");
        println!("3. Изменить настройки Gitea");
        println!("4. Изменить расписание бэкапов");
        println!("5. Изменить имя бэкапа");
        println!("6. Выход");

        match read_input("\nВыберите действие (1-6): ")?.as_str() {
            "1" => perform_backup(&mut config)?,
            "2" => manage_backup_paths(&mut config)?,
            "3" => setup_gitea(&mut config)?,
            "4" => setup_backup_schedule(&mut config)?,
            "5" => setup_backup_name(&mut config)?,
            "6" => break,
            _ => println!("Неверный выбор, попробуйте снова"),
        }
    }

    Ok(())
}
