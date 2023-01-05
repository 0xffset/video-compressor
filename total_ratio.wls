#!/usr/bin/env wolframscript
SetOptions[$Output, FormatType -> OutputForm];
data = Last@First@Import@ToString@StringForm["``/compression_log.json", First@Rest@$ScriptCommandLine];
prev = UnitConvert[Quantity[Total@Map[Last@#[[1]] &, Map[Last, data]], "Bytes"], "Gigabytes"] // N;
post = UnitConvert[Quantity[Total@Map[Last@#[[2]] &, Map[Last, data]], "Bytes"], "Gigabytes"] // N;
Print@StringForm["Compressed `` to ``, using `` of the previous storage", prev, post, PercentForm[post/prev]];
