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
export const addProject = (path: string) => invoke<Project>("project_add", { path });
export const getAppInfo = () => invoke<AppInfo>("app_info");
