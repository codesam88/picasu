export const isPasswordChangeSubmittable = (
  newPassword: string,
  confirmPassword: string
): boolean => {
  return newPassword.length > 0 && newPassword === confirmPassword
}

export const submitPasswordChange = async (
  oldPassword: string,
  newPassword: string,
  confirmPassword: string,
  update: (oldPassword: string, newPassword: string) => Promise<void>
): Promise<boolean> => {
  if (!isPasswordChangeSubmittable(newPassword, confirmPassword)) return false
  await update(oldPassword, newPassword)
  return true
}
