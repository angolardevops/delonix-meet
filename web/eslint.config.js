/**
 * ESLint do frontend — e a razão de existir.
 *
 * O código tinha **52 `// eslint-disable-next-line react-hooks/exhaustive-deps`**
 * e **nenhum ESLint**: comentários decorativos a silenciar uma regra que nada
 * aplicava. É o que deixou passar os efeitos que apagam o que o utilizador está
 * a escrever (o cartão de definições da organização, o campo de timecode do
 * editor) e os `matchMedia` lidos uma vez.
 *
 * O que está ligado é **deliberadamente pouco**: as regras dos hooks, que são
 * as que o React não perdoa, mais o que o `tsc` não vê. Tudo o resto fica de
 * fora — um lint que grita por estilo é um lint que se desliga, e já houve 52
 * provas disso neste repo.
 *
 * O portão NÃO exige zero: exige que o número não suba
 * (`scripts/check-frontend-lint.sh` contra `scripts/eslint-baseline.txt`), como
 * a catraca do clippy no backend. Um número que só desce fecha a porta sem
 * parar o trabalho.
 */
import js from '@eslint/js'
import reactHooks from 'eslint-plugin-react-hooks'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  {
    ignores: ['dist/**', 'node_modules/**', 'public/**', 'e2e/**', '*.config.js', '*.config.ts'],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    plugins: { 'react-hooks': reactHooks },
    languageOptions: {
      parserOptions: { ecmaFeatures: { jsx: true } },
      globals: {
        window: 'readonly',
        document: 'readonly',
        navigator: 'readonly',
        location: 'readonly',
        history: 'readonly',
        console: 'readonly',
        fetch: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        setInterval: 'readonly',
        clearInterval: 'readonly',
        requestAnimationFrame: 'readonly',
        cancelAnimationFrame: 'readonly',
        queueMicrotask: 'readonly',
      },
    },
    rules: {
      // AS DUAS QUE IMPORTAM.
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      // O `tsc` já apanha tipos; estas são as que ele não vê e que mordem.
      'no-constant-condition': ['warn', { checkLoops: false }],
      // Desligadas de propósito: barulho sem defeito atrás.
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-unused-vars': 'off', // o `tsc` com noUnusedLocals já o faz
      '@typescript-eslint/no-empty-object-type': 'off',
      'no-empty': 'off',
      'no-undef': 'off', // o TypeScript resolve os símbolos
    },
  },
  {
    // `src/cena/` é FOLHA: não importa de mais nenhuma pasta de `src/`.
    //
    // PORQUÊ. A moldura de transmissão é desenhada pelos dois compositores — o
    // da Sala (`room/compositor.ts`) e o do Estúdio (`studio/compositor.ts`) —
    // logo vive à parte, para não ser escrita duas vezes. O caminho preguiçoso
    // para a construir era importar dali o que ela precisasse (o `Rect` nascia
    // em `room/compositor.ts`), e nesse instante ficava `room → cena → room`.
    // O TypeScript compila ciclos de tipo calado, e o Vite resolve muitos em
    // silêncio até ao dia em que um módulo é avaliado antes de estar pronto e
    // alguém recebe `undefined` no sítio de uma função.
    //
    // PORQUE É `../*` E NÃO UMA LISTA DE PASTAS. A primeira versão desta regra
    // era um script de `grep` que proibia `room/` e `studio/` por nome. Tinha
    // cinco buracos, todos medidos: importações multi-linha eram invisíveis (o
    // `from` fica numa linha sem a palavra `import`, e há 62 dessas no repo);
    // `'../room'` sem barra final escapava; `@mediapipe/studio/core` era
    // acusado por engano; ficheiros `.js` e `.spec.ts` não eram lidos. E o
    // pior: ciclos TRANSITIVOS passavam incólumes — `media/correccaoGl.ts` já
    // importa de `studio/`, portanto `cena → media → studio → room → cena`
    // fechava-se com o portão a dar verde.
    //
    // Proibir `../` inteiro não tem nenhum desses buracos: é resolvido pelo
    // ESLint sobre a árvore sintáctica, apanha qualquer forma de import, e a
    // folha deixa de poder alcançar o que quer que seja fora de si. A cena
    // continua livre de importar pacotes npm e os seus próprios irmãos (`./`).
    files: ['src/cena/**/*.{ts,tsx,js}'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: ['../*'],
              message:
                'src/cena/ é folha: não pode importar de outra pasta de src/. ' +
                'Move para cena/ o que precisares e re-exporta-o do lado de lá ' +
                '(foi o que se fez ao Rect), senão fecha-se um ciclo de módulos.',
            },
          ],
        },
      ],
    },
  },
  {
    // Os AudioWorklets correm NOUTRO contexto (`AudioWorkletGlobalScope`), com
    // globais próprias que o browser lhes dá. Não são erros: é outro mundo.
    files: ['**/*Worklet.js'],
    languageOptions: {
      globals: {
        AudioWorkletProcessor: 'readonly',
        registerProcessor: 'readonly',
        sampleRate: 'readonly',
        currentTime: 'readonly',
      },
    },
  },
)
