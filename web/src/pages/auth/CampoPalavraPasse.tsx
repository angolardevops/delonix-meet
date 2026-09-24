/** Palavra-passe com o botão de a mostrar — escrever às cegas num telemóvel é o erro mais comum. */
import { InputHTMLAttributes, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { IconButton, TextInput } from '../../ui/kit'

export default function CampoPalavraPasse(props: Omit<InputHTMLAttributes<HTMLInputElement>, 'type'>) {
  const { t } = useTranslation()
  const [visivel, setVisivel] = useState(false)
  return (
    <div className="auth-pass">
      <TextInput {...props} large type={visivel ? 'text' : 'password'} />
      <IconButton
        icon={visivel ? 'eyeOff' : 'eye'}
        label={visivel ? t('auth.entrar.esconder') : t('auth.entrar.mostrar')}
        bare
        aria-pressed={visivel}
        onClick={() => setVisivel((v) => !v)}
      />
    </div>
  )
}
