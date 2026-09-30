import { type ReactNode, useEffect } from 'react'
import { Icon, type IconName } from './Icon.tsx'

/** A modal "are you sure?" in ergo's own style. Escape or the backdrop cancels. */
export function Confirm({
  icon = 'play',
  title,
  children,
  confirmLabel,
  danger,
  onConfirm,
  onCancel,
}: {
  icon?: IconName
  title: ReactNode
  children?: ReactNode
  confirmLabel: string
  /** Destructive action: red icon and button. */
  danger?: boolean
  onConfirm: () => void
  onCancel: () => void
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onCancel()
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onCancel])

  return (
    <div className="backdrop" onClick={onCancel}>
      <div
        className={`dialog${danger ? ' danger' : ''}`}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        onClick={(e) => e.stopPropagation()}
      >
        <span className="dialog-icon">
          <Icon name={icon} size={26} />
        </span>
        <h2 id="confirm-title">{title}</h2>
        {children && <div className="dialog-text">{children}</div>}
        <div className="dialog-actions">
          {/* For destructive actions, Enter shouldn't be the one that deletes. */}
          <button className="btn big" autoFocus={danger} onClick={onCancel}>
            Cancel
          </button>
          <button className={`btn big ${danger ? 'danger-solid' : 'primary'}`} autoFocus={!danger} onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  )
}
