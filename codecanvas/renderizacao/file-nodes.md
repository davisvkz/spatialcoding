# Nós de arquivo

- **Texto** [bundle]: textura de canvas com syntax highlight; fonte "Monaco, Consolas, monospace" 11 px, linha 18 px, largura padrão 650 (Settings: 700; 300–2100; fonte 8–20). Texturas em chunks de 2048 px.
- **Minimap** [bundle]: um retângulo por trecho não branco, na cor do token, `globalAlpha .4`, altura `lineHeight-2`.
- **Header/label**: nome do arquivo acima do nó; em zoom médio vira pill que gruda no topo da viewport [obs]. Fade entre 0.04 e 0.01.
- **Semantic fill** (L5): nó inteiro preenchido com a cor de seleção, diff ou busca.
- **Long files**: 1000 primeiras linhas + linha "Long file" clicável; símbolos recortados; edges da cauda ligam ao marcador.
- **Gutter**: números de linha à esquerda; a linha sob o cursor ganha faixa destacada [obs].
- Seleção: borda azul no nó; linha/statement clicado ganha contorno [obs].
- Origem do conteúdo de edges: topo do nó = `CONTENT_ORIGIN_Y = 40`; y de linha = `y + 40 + (linha-1)*lineHeight + centro` [bundle].
