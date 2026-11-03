import { useState } from "react";

import { useWorkspace } from "../../store/workspace";

/**
 * 人物列表：谁在哪几章出场。
 *
 * 数据来自每章 front matter 的 `characters` / `pov`，没有独立的角色文件要维护。
 * 点角色展开出场章节，点章节直接跳过去。
 */
export function CharacterList() {
  const characters = useWorkspace((s) => s.characters);
  const openDoc = useWorkspace((s) => s.openDoc);
  const activePath = useWorkspace((s) => s.activePath);

  /** 同时只展开一个角色，避免列表被撑得过长 */
  const [expanded, setExpanded] = useState<string | null>(null);

  if (characters.length === 0) {
    return (
      <div className="explorer-empty">
        <p>还没有记录任何人物。</p>
        <p className="hint">
          在章节的 front matter 里写 <code>characters: [沈砚]</code> 或{" "}
          <code>pov: 沈砚</code>，这里就会自动汇总。
        </p>
      </div>
    );
  }

  return (
    <div className="characters">
      {characters.map((card) => {
        const isOpen = expanded === card.name;

        return (
          <div key={card.name} className={`character-item${isOpen ? " is-open" : ""}`}>
            <button
              type="button"
              className="character-head"
              title={`${card.name}：出场 ${card.appearances.length} 章`}
              onClick={() => setExpanded(isOpen ? null : card.name)}
            >
              <span className="character-arrow">{isOpen ? "▾" : "▸"}</span>
              <span className="character-name">{card.name}</span>
              <span className="character-meta">
                {card.povCount > 0 && (
                  <span className="character-pov" title={`作为视角人物 ${card.povCount} 章`}>
                    视角 {card.povCount}
                  </span>
                )}
                <span className="character-count">{card.appearances.length}</span>
              </span>
            </button>

            {isOpen && (
              <div className="character-appearances">
                {card.appearances.map((a) => (
                  <button
                    key={a.path}
                    type="button"
                    className={`character-chapter${a.path === activePath ? " is-active" : ""}`}
                    title={a.path}
                    onClick={() => void openDoc(a.path)}
                  >
                    <span className="character-chapter-title">{a.title}</span>
                    {a.dir && <span className="character-chapter-dir">{a.dir}</span>}
                    {a.isPov && (
                      <span className="character-chapter-pov" title="本章视角">
                        ◉
                      </span>
                    )}
                  </button>
                ))}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
