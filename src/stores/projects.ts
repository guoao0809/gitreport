import { defineStore } from "pinia";
import { ref, watch } from "vue";
import type { Project, DetectResult } from "../types";

const STORAGE_KEY = "gitreport-projects";

/** 从 localStorage 读项目列表，无数据返回空数组 */
function readProjects(): Project[] {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) return [];
  try {
    return JSON.parse(raw) as Project[];
  } catch {
    return [];
  }
}

export const useProjectStore = defineStore("projects", () => {
  const projects = ref<Project[]>([]);

  // 状态变化自动持久化
  watch(
    projects,
    (v) => localStorage.setItem(STORAGE_KEY, JSON.stringify(v)),
    { deep: true },
  );

  /** 应用启动时从 localStorage 恢复 */
  function load() {
    projects.value = readProjects();
  }

  /**
   * 把扫描到的仓库导入项目列表。
   * paths 是用户勾选的目录，detected 是 detectGitRepos 的完整结果。
   * 已存在的 path 跳过，返回新导入的数量。
   */
  function addProjects(paths: string[], detected: DetectResult[]): number {
    const byPath = new Map(detected.map((d) => [d.path, d]));
    const existing = new Set(projects.value.map((p) => p.path));
    let added = 0;
    for (const path of paths) {
      const d = byPath.get(path);
      if (!d || existing.has(path)) continue;
      projects.value.push({
        id: path,
        name: d.name,
        path: d.path,
        branch: d.branch,
        importedAt: Date.now(),
        lastCommitAt: d.lastCommitAt,
        myCommitCount: 0,
      });
      existing.add(path);
      added++;
    }
    return added;
  }

  /** 按 id 移除项目 */
  function removeProject(id: string) {
    projects.value = projects.value.filter((p) => p.id !== id);
  }

  /** 更新项目要统计的分支 */
  function updateBranch(id: string, branch: string) {
    const p = projects.value.find((x) => x.id === id);
    if (p) p.branch = branch;
  }

  /** 设置/清空项目别称（AI 生成时按别称展示） */
  function setProjectAlias(id: string, alias: string) {
    const p = projects.value.find((x) => x.id === id);
    if (p) p.alias = alias;
  }

  /** 判断某路径是否已导入 */
  function hasProject(path: string): boolean {
    return projects.value.some((p) => p.path === path);
  }

  return { projects, load, addProjects, removeProject, updateBranch, setProjectAlias, hasProject };
});
