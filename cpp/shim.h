#pragma once

#include "rust/cxx.h"

int run(bool visible, rust::Str url);
void app_quit();
bool window_active();
rust::String clipboard_text();
void set_clipboard(rust::Str text);
rust::String ui_font();
void set_color_scheme(bool dark);
rust::String fmt_ts(int64_t ts, uint8_t kind);
int64_t days_ago(int64_t ts);
bool save_icon(rust::Slice<const uint8_t> data, rust::Str path);
