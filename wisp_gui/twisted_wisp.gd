extends Control

const FlowGraphView = preload("res://flow_graph_view.tscn")

var wisp: TwistedWisp = null

func _ready():
	var config = FileAccess.open("res://wisp.toml", FileAccess.READ)
	var config_text = config.get_as_text()
	wisp = TwistedWisp.create(config_text)
	var graph = FlowGraphView.instantiate()
	graph.wisp = wisp
	add_child(graph)
	graph.grab_focus()


func _process(_delta):
	pass
