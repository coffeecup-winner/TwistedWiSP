extends FlowGraphNode


func _ready():
	super._ready()
	flow_node.add_watch()


func _process(_delta):
	var values = flow_node.get_watch_updates()
	if len(values) > 0:
		$Value.text = str(values[-1])
