import { ReactNode, useCallback, useEffect, useRef, useState } from 'react'
import { cx } from '../ui/kit'

/**
 * Menu flutuante ancorado a um botão da barra. Fecha com Esc (devolvendo o
 * foco ao botão) e com um clique fora. Não é modal: os controlos da chamada
 * continuam alcançáveis.
 */
export function usePopover() {
  const [open, setOpen] = useState(false)
  const wrapRef = useRef<HTMLDivElement>(null)
  const close = useCallback(() => setOpen(false), [])
  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      setOpen(false)
      wrapRef.current?.querySelector<HTMLElement>('[aria-haspopup]')?.focus()
    }
    document.addEventListener('pointerdown', onDown)
    window.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [open])
  return { open, setOpen, toggle: () => setOpen((v) => !v), close, wrapRef }
}

export function PopoverPanel({
  label,
  className,
  role = 'dialog',
  align = 'center',
  children,
}: {
  label: string
  className?: string
  role?: 'dialog' | 'menu'
  align?: 'center' | 'start' | 'end'
  children: ReactNode
}) {
  return (
    <div className={cx('rm-pop', `rm-pop--${align}`, className)} role={role} aria-label={label}>
      {children}
    </div>
  )
}

/** Uma linha de menu, com visto opcional (menu de opções da barra). */
export function MenuItem({
  icon,
  checked,
  onClick,
  children,
  className,
  disabled,
}: {
  icon?: ReactNode
  checked?: boolean
  onClick: () => void
  children: ReactNode
  className?: string
  disabled?: boolean
}) {
  return (
    <button
      type="button"
      role={checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
      aria-checked={checked === undefined ? undefined : checked}
      className={cx('rm-menu__item', className)}
      onClick={onClick}
      disabled={disabled}
    >
      <span className="rm-menu__icon" aria-hidden="true">
        {icon}
      </span>
      <span className="rm-menu__text">{children}</span>
      {checked !== undefined && <span className={cx('rm-menu__check', checked && 'is-on')} aria-hidden="true" />}
    </button>
  )
}
