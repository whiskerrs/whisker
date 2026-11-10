#include <windows.h>

#include "whisker/whisker_app.h"

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
  static constexpr char kTitle[] = "Whisker";
  WhiskerWindowsAppOptions options{kTitle, sizeof(kTitle) - 1, {255, 255, 255}};
  return whisker_windows_run(&options) < 0 ? 1 : 0;
}
