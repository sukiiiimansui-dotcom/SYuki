import { invoke } from '@tauri-apps/api/core'

/** 角色记忆库视图（对应后端 `get_role_memory_bank` 的返回）。 */
export interface RoleMemoryView {
  role_id: number
  role_name: string
  /** 永久记忆是否开启（全局配置 use_persistent_memory）。 */
  memory_enabled: boolean
  schema_version: number
  updated_at: string
  /** 短期上下文摘要（近期回顾 / 承接话题）。 */
  short_term: string
  /** 长期经历编年史（关键事件）。 */
  long_term: string
  /** 用户信息（ta 的画像：姓名/年龄/喜好/雷点）。 */
  user_info: string
  /** 待办与契约清单（重要约定）。 */
  promises: string
}

/** 读取指定角色的记忆库（MemoryBank），供记忆可视化面板展示。 */
export const getRoleMemoryBank = (roleId: number): Promise<RoleMemoryView> =>
  invoke<RoleMemoryView>('get_role_memory_bank', { roleId })
