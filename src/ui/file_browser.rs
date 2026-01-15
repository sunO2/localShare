//! 文件浏览器组件

use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// 文件或目录项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileItem {
    /// 名称
    pub name: String,
    /// 完整路径
    pub path: PathBuf,
    /// 是否为目录
    pub is_dir: bool,
    /// 文件大小（字节）
    pub size: u64,
}

impl FileItem {
    /// 从路径创建文件项
    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?.to_string();
        let metadata = fs::metadata(path).ok()?;

        Some(FileItem {
            name,
            path: path.to_path_buf(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
        })
    }

    /// 获取显示字符串
    pub fn display_name(&self) -> String {
        if self.is_dir {
            format!("📁 {}", self.name)
        } else {
            format!("📄 {}", self.name)
        }
    }
}

/// 文件浏览器状态
#[derive(Debug, Clone)]
pub struct FileBrowser {
    /// 当前目录
    current_dir: PathBuf,
    /// 文件列表
    files: Vec<FileItem>,
    /// 选中的索引
    selected_index: usize,
    /// 历史记录（用于返回上级）
    history: Vec<PathBuf>,
    /// 历史记录索引
    history_index: usize,
}

impl FileBrowser {
    /// 创建新的文件浏览器
    pub fn new(start_dir: PathBuf) -> Self {
        let mut browser = FileBrowser {
            current_dir: start_dir.clone(),
            files: Vec::new(),
            selected_index: 0,
            history: vec![start_dir],
            history_index: 0,
        };
        browser.refresh();
        browser
    }

    /// 刷新当前目录
    pub fn refresh(&mut self) {
        self.files = self.read_directory(&self.current_dir);
        self.selected_index = 0;
    }

    /// 读取目录内容
    fn read_directory(&self, path: &Path) -> Vec<FileItem> {
        let mut items = Vec::new();

        if let Ok(entries) = fs::read_dir(path) {
            let mut dirs = Vec::new();
            let mut files = Vec::new();

            for entry in entries.filter_map(|e| e.ok()) {
                if let Some(item) = FileItem::from_path(&entry.path()) {
                    if item.is_dir {
                        dirs.push(item);
                    } else {
                        files.push(item);
                    }
                }
            }

            // 排序：目录在前，文件在后
            dirs.sort_by(|a, b| a.name.cmp(&b.name));
            files.sort_by(|a, b| a.name.cmp(&b.name));

            items.extend(dirs);
            items.extend(files);
        }

        items
    }

    /// 进入目录
    pub fn enter_directory(&mut self) -> bool {
        if self.selected_index >= self.files.len() {
            return false;
        }

        let item = &self.files[self.selected_index];

        if !item.is_dir {
            return false;
        }

        let new_dir = item.path.clone();

        // 更新历史记录
        if self.history_index < self.history.len() - 1 {
            self.history.truncate(self.history_index + 1);
        }
        self.history.push(new_dir.clone());
        self.history_index = self.history.len() - 1;

        self.current_dir = new_dir;
        self.refresh();
        true
    }

    /// 返回上级目录
    pub fn go_up(&mut self) -> bool {
        if let Some(parent) = self.current_dir.parent() {
            let parent_dir = parent.to_path_buf();

            // 更新历史记录
            if self.history_index < self.history.len() - 1 {
                self.history.truncate(self.history_index + 1);
            }
            self.history.push(parent_dir.clone());
            self.history_index = self.history.len() - 1;

            self.current_dir = parent_dir;
            self.refresh();
            true
        } else {
            false
        }
    }

    /// 向后导航
    pub fn go_back(&mut self) -> bool {
        if self.history_index > 0 {
            self.history_index -= 1;
            self.current_dir = self.history[self.history_index].clone();
            self.refresh();
            true
        } else {
            false
        }
    }

    /// 向前导航
    pub fn go_forward(&mut self) -> bool {
        if self.history_index + 1 < self.history.len() {
            self.history_index += 1;
            self.current_dir = self.history[self.history_index].clone();
            self.refresh();
            true
        } else {
            false
        }
    }

    /// 选择上一项
    pub fn select_previous(&mut self) {
        if !self.files.is_empty() && self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// 选择下一项
    pub fn select_next(&mut self) {
        if !self.files.is_empty() && self.selected_index + 1 < self.files.len() {
            self.selected_index += 1;
        }
    }

    /// 选择第一项
    pub fn select_first(&mut self) {
        self.selected_index = 0;
    }

    /// 选择最后一项
    pub fn select_last(&mut self) {
        if !self.files.is_empty() {
            self.selected_index = self.files.len() - 1;
        }
    }

    /// 获取当前目录
    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    /// 获取文件列表
    pub fn files(&self) -> &[FileItem] {
        &self.files
    }

    /// 获取选中索引
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// 获取选中的文件
    pub fn selected_file(&self) -> Option<&FileItem> {
        self.files.get(self.selected_index)
    }

    /// 是否可以返回上级
    pub fn can_go_up(&self) -> bool {
        self.current_dir.parent().is_some()
    }

    /// 是否可以后退
    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    /// 是否可以前进
    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }
}
