// port of core/domain/icons.ts's IconName string union.

enum IconName {
  play("play"),
  pause("pause"),
  stop("stop"),
  restart("restart"),
  replay("replay"),
  step("step"),
  continue_("continue"),
  close("close"),
  x("x"),
  plus("plus"),
  minus("minus"),
  trash("trash"),
  edit("edit"),
  save("save"),
  download("download"),
  upload("upload"),
  check("check"),
  alert("alert"),
  info("info"),
  search("search"),
  file("file"),
  folder("folder"),
  bell("bell"),
  settings("settings"),
  refresh("refresh"),
  debug("debug"),
  mail("mail"),
  approve("approve"),
  reject("reject"),
  arrowUp("arrow-up"),
  arrowDown("arrow-down"),
  chevronLeft("chevron-left"),
  chevronRight("chevron-right"),
  workflow("workflow"),
  runs("runs"),
  list("list"),
  key("key"),
  lock("lock"),
  box("box"),
  message("message"),
  gate("gate"),
  gear("gear"),
  flag("flag"),
  tag("tag"),
  cursor("cursor"),
  skip("skip"),
  circle("circle"),
  dot("dot"),
  breakpoint("breakpoint"),
  bolt("bolt"),
  clock("clock"),
  hourglass("hourglass"),
  branch("branch"),
  switch_("switch"),
  toggle("toggle"),
  percentage("percentage"),
  loop("loop"),
  parallel("parallel"),
  join("join"),
  shield("shield"),
  user("user"),
  grid("grid"),
  race("race"),
  emit("emit"),
  output("output");

  const IconName(this.wire);

  final String wire;

  static IconName? fromWire(String? value) {
    if (value == null) {
      return null;
    }

    for (final name in IconName.values) {
      if (name.wire == value) {
        return name;
      }
    }

    return null;
  }
}
