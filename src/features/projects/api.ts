import { invoke } from "@tauri-apps/api/core";

export interface Project {
  id: string;
  name: string;
  primary_root: string;
  created_at: string;
}

export interface AppInfo {
  version: string;
  database_path: string;
}

export const listProjects = () => invoke<Project[]>("project_list");
export const addProject = (path: string, name?: string) => invoke<Project>("project_add", { path, name: name?.trim() ? name.trim() : null });
export const renameProject = (id: string, name?: string) => invoke<Project>("project_rename", { id, name: name?.trim() ? name.trim() : null });
export const removeProject = (id: string) => invoke<void>("project_remove", { id });
export const openProjectDirectory = (id: string) => invoke<void>("project_open_directory", { id });
export const getAppInfo = () => invoke<AppInfo>("app_info");
