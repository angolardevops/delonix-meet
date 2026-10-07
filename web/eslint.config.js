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
