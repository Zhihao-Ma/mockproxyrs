// Copyright 2024 mazao
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod commands;
mod state;

use commands::*;
use log::info;
use mockproxyrs_core::mock::ScriptEngine;
use mockproxyrs_core::repository::SqliteRepository;
use state::AppState;
use std::time::Duration;

/// 初始化日志
fn init_log(log_dir: &std::path::Path) {
    // 创建日志目录
    let _ = std::fs::create_dir_all(log_dir);

    // 日志文件路径
    let log_file = log_dir.join("mockproxyrs.log");

    // 初始化 fast_log
    fast_log::init(
        fast_log::config::Config::new()
            .chan_len(Some(100000))
            .level(log::LevelFilter::Info)
            .split::<fast_log::plugin::file_split::RawFile, _, _, _>(
                log_file.to_str().unwrap(),
                fast_log::plugin::file_split::KeepType::KeepTime(Duration::from_secs(
                    30 * 24 * 60 * 60,
                )),
                fast_log::plugin::packer::LogPacker {},
                fast_log::plugin::file_split::Rolling::new(
                    fast_log::plugin::file_split::RollingType::BySize(
                        fast_log::consts::LogSize::MB(1),
                    ),
                ),
            ),
    )
    .unwrap();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 获取数据目录
    let data_dir = dirs::data_local_dir()
        .map(|p| p.join("mockproxyrs"))
        .expect("Failed to get data directory");

    // 确保目录存在
    std::fs::create_dir_all(&data_dir).expect("Failed to create data directory");

    // 初始化日志
    let log_dir = data_dir.join("logs");
    init_log(&log_dir);
    info!("Mockproxyrs starting...");

    // 初始化数据库
    let db_path = data_dir.join("data.db");
    info!("Database path: {:?}", db_path);

    let repository = SqliteRepository::new(&db_path).expect("Failed to initialize database");
    let script_engine = ScriptEngine::new().expect("Failed to initialize script engine");
    let state = AppState::new(repository, script_engine);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            // 服务管理
            list_services,
            list_service_status,
            get_service,
            add_service,
            update_service,
            delete_service,
            start_service,
            stop_service,
            // 规则管理
            list_rules,
            add_rule,
            update_rule,
            delete_rule,
            delete_rules_by_service,
            // 事件通道
            create_channel,
            destroy_channel,
            // 脚本校验
            validate_script,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
