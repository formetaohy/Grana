import os

application = defines.get("app")
background_image = defines.get("background")

appname = os.path.basename(application)

volume_name = "Grana"
format = "UDZO"
files = [application]
symlinks = {"Applications": "/Applications"}
icon_locations = {appname: (180, 190), "Applications": (480, 190)}
background = background_image
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
window_rect = ((220, 220), (660, 360))
default_view = "icon-view"
show_icon_preview = False
arrange_by = None
grid_offset = (0, 0)
label_pos = "bottom"
text_size = 13
icon_size = 128

