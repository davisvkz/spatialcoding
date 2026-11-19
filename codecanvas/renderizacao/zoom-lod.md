# Níveis de zoom (LOD)

Painel **Shift+V** (barra L5…L1, com zoom, pos, screen e visible; botão de reset). Zoom vai de 0.001 a 1 (máx 100%); `F` zooma para 60% no mouse.

| Nível | Zoom | Texto do arquivo | CST | Label do arquivo |
|---|---|---|---|---|
| L1 | ≥ 0.30 | código colorido legível, hit por caractere | sem tinta base (só hover/seleção) | visível; vira "pill" fixo no topo quando o topo do nó sai da tela [obs] |
| L2 | ≥ 0.10 | "minimap": retângulos coloridos por token, alpha .4 | tinta base alpha .04 | visível |
| L3 | ≥ 0.04 | nenhum | alpha .10 | visível |
| L4 | ≥ 0.01 | nenhum | .10 | fade (0.04 → 0.01) |
| L5 | < 0.01 | nenhum | .10 | oculto; **semantic fill** (nó preenchido por seleção/diff/busca) |

[bundle] `ZOOM_LEVEL_THRESHOLD_*`; `renderModeByLevel = text, minimap, none, none, none`; `cstBaseModeByLevel = text, minimap, minimap, minimap, none`; `headerLabelsByLevel = visible, visible, visible, fade, hidden`. `normalizeConfig` aplica esses limiares a todas as features.

Observado [obs]: 0.60x código nítido (`img/19`); 0.45–0.34x texto miúdo com pills de label; 0.26x tokens em retângulos e regiões CST tintadas (`img/11`); 0.13x blocos de linha (`img/12`).
Capturas: `img/10`, `11`, `12`, `19`.
