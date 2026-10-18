package main

import (
	"fmt"
	"strconv"
)

type Particle struct {
	x, y, vx, vy, mass float64
}

func (self Particle) advance(dt float64) Particle {
	return Particle{
		x:    self.x + self.vx*dt,
		y:    self.y + self.vy*dt,
		vx:   self.vx - self.x*dt*0.001,
		vy:   self.vy - self.y*dt*0.001,
		mass: self.mass,
	}
}

func (self Particle) energy() float64 {
	return 0.5 * self.mass * (self.vx*self.vx + self.vy*self.vy)
}

func main() {
	const count int64 = 50000
	const steps int64 = 3000
	const dt float64 = 0.01

	total := 0.0
	var seed uint64 = 88172645463325252

	for p := int64(0); p < count; p++ {
		seed = seed*6364136223846793005 + 1442695040888963407
		sx := float64((seed>>33)%1000) * 0.001
		seed = seed*6364136223846793005 + 1442695040888963407
		sy := float64((seed>>33)%1000) * 0.001

		part := Particle{
			x:    sx,
			y:    sy,
			vx:   1.0 - sy,
			vy:   1.0 - sx,
			mass: 1.0 + sx,
		}

		for s := int64(0); s < steps; s++ {
			part = part.advance(dt)
		}
		total = total + part.energy() + part.x - part.y
	}

	fmt.Println(strconv.FormatFloat(total, 'f', -1, 64))
}
