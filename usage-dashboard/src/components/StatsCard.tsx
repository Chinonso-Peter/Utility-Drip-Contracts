'use client';

import { LucideIcon, TrendingUp, TrendingDown, Zap, DollarSign } from 'lucide-react';

interface StatsCardProps {
  title: string;
  value: string;
  unit: string;
  change?: number;
  icon: LucideIcon;
  trend?: 'up' | 'down' | 'neutral';
  isHighlighted?: boolean;
}

export default function StatsCard({ 
  title, 
  value, 
  unit, 
  change, 
  icon: Icon, 
  trend = 'neutral',
  isHighlighted = false 
}: StatsCardProps) {
  const TrendIcon = trend === 'up' ? TrendingUp : trend === 'down' ? TrendingDown : null;
  
  return (
    <div className={`stat-card ${isHighlighted ? 'ring-2 ring-primary-500' : ''}`}>
      <div className="flex items-center justify-between">
        <div className="flex items-center space-x-3">
          <div className={`p-2 rounded-lg ${isHighlighted ? 'bg-primary-100' : 'bg-gray-100'}`}>
            <Icon className={`w-5 h-5 ${isHighlighted ? 'text-primary-600' : 'text-gray-600'}`} />
          </div>
          <div>
            <p className="text-sm font-medium text-gray-600">{title}</p>
            <p className="text-2xl font-bold text-gray-900">
              {value}
              <span className="text-sm font-normal text-gray-500 ml-1">{unit}</span>
            </p>
          </div>
        </div>
        
        {change !== undefined && TrendIcon && (
          <div className={`flex items-center space-x-1 ${
            trend === 'up' ? 'text-green-600' : 'text-red-600'
          }`}>
            <TrendIcon className="w-4 h-4" />
            <span className="text-sm font-medium">{Math.abs(change)}%</span>
          </div>
        )}
      </div>
    </div>
  );
}
